use std::{alloc, error, fmt, io, mem, process, ptr};

pub trait BoxExt<T>: Sized {
    fn try_box(value: T) -> Result<Self, AllocationError>;
}

pub trait BoxSliceExt<T>: Sized {
    fn try_box_with(
        len: usize,
        initialize: impl FnMut(usize) -> T,
    ) -> Result<Self, AllocationError>;
}

pub(crate) trait BoxUninitExt<T>: Sized {
    fn try_box_uninit(len: usize) -> Result<Self, AllocationError>;
}

pub trait VecExt<T>: Sized {
    fn try_vec_with_capacity(capacity: usize) -> Result<Self, AllocationError>;
}

/// Failure to reserve a fixed collection's complete backing allocation.
#[derive(Clone, Copy, Debug)]
pub struct AllocationError {
    layout: Option<alloc::Layout>,
}

impl AllocationError {
    fn overflow() -> Self {
        Self { layout: None }
    }

    fn exhausted(layout: alloc::Layout) -> Self {
        Self {
            layout: Some(layout),
        }
    }

    pub(crate) fn abort(self) -> ! {
        match self.layout {
            Some(layout) => alloc::handle_alloc_error(layout),
            None => process::abort(),
        }
    }
}

impl fmt::Display for AllocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.layout {
            Some(_) => formatter.write_str("collection allocation failed"),
            None => formatter.write_str("collection allocation layout exceeds the platform limit"),
        }
    }
}

impl error::Error for AllocationError {}

impl From<AllocationError> for io::Error {
    fn from(_: AllocationError) -> Self {
        io::ErrorKind::OutOfMemory.into()
    }
}

struct Initialized<'a, T> {
    entries: &'a mut [mem::MaybeUninit<T>],
    len: usize,
}

impl<T> Drop for Initialized<'_, T> {
    fn drop(&mut self) {
        for entry in &mut self.entries[..self.len] {
            unsafe { entry.assume_init_drop() };
        }
    }
}

impl<T> BoxUninitExt<T> for Box<[mem::MaybeUninit<T>]> {
    fn try_box_uninit(len: usize) -> Result<Box<[mem::MaybeUninit<T>]>, AllocationError> {
        let layout = alloc::Layout::array::<mem::MaybeUninit<T>>(len)
            .map_err(|_| AllocationError::overflow())?;
        let data = if layout.size() == 0 {
            ptr::NonNull::dangling()
        } else {
            // SAFETY: `layout` is nonzero and was derived for exactly `len` entries.
            let data = unsafe { alloc::alloc(layout) }.cast::<mem::MaybeUninit<T>>();
            ptr::NonNull::new(data).ok_or_else(|| AllocationError::exhausted(layout))?
        };
        let slice = ptr::slice_from_raw_parts_mut(data.as_ptr(), len);
        // SAFETY: `data` is either the correctly aligned dangling pointer for an
        // empty/ZST slice or a unique allocation for this exact slice layout.
        Ok(unsafe { Box::from_raw(slice) })
    }
}

impl<T> BoxSliceExt<T> for Box<[T]> {
    /// Allocates and initializes one exact boxed slice without invoking the global
    /// allocation-error handler.
    fn try_box_with(
        len: usize,
        mut initialize: impl FnMut(usize) -> T,
    ) -> Result<Box<[T]>, AllocationError> {
        let mut entries: Box<[mem::MaybeUninit<T>]> = BoxUninitExt::try_box_uninit(len)?;
        {
            let mut initialized = Initialized {
                entries: &mut entries,
                len: 0,
            };
            for index in 0..len {
                initialized.entries[index].write(initialize(index));
                initialized.len += 1;
            }
            initialized.len = 0;
        }
        // SAFETY: the loop initialized every entry and disarmed the unwind guard.
        Ok(unsafe { entries.assume_init() })
    }
}

impl<T> BoxExt<T> for Box<T> {
    fn try_box(value: T) -> Result<Self, AllocationError> {
        let layout = alloc::Layout::new::<T>();
        let entry = if layout.size() == 0 {
            ptr::NonNull::<T>::dangling().as_ptr()
        } else {
            let entry = unsafe { alloc::alloc(layout) }.cast::<T>();
            ptr::NonNull::new(entry)
                .ok_or_else(|| AllocationError::exhausted(layout))?
                .as_ptr()
        };
        unsafe { entry.write(value) };
        Ok(unsafe { Box::from_raw(entry) })
    }
}

impl<T> VecExt<T> for Vec<T> {
    fn try_vec_with_capacity(capacity: usize) -> Result<Self, AllocationError> {
        let layout =
            alloc::Layout::array::<T>(capacity).map_err(|_| AllocationError::overflow())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(capacity)
            .map_err(|_| AllocationError::exhausted(layout))?;
        Ok(values)
    }
}

pub(crate) struct ClearGuard<'a, T: ?Sized> {
    value: &'a mut T,
    clear: fn(&mut T),
    armed: bool,
}

impl<'a, T: ?Sized> ClearGuard<'a, T> {
    pub(crate) fn run(value: &'a mut T, clear: fn(&mut T)) {
        let mut guard = Self {
            value,
            clear,
            armed: true,
        };
        (guard.clear)(guard.value);
        guard.armed = false;
    }
}

impl<T: ?Sized> Drop for ClearGuard<'_, T> {
    fn drop(&mut self) {
        if self.armed {
            (self.clear)(self.value);
        }
    }
}
