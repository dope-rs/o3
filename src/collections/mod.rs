pub mod arena;
pub mod batch;
pub mod fixed;
pub mod heap;
pub mod intrusive;
pub mod pinned;
pub mod queue;
pub mod slab;

use std::{alloc, error, fmt, mem, ops, ptr};

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
            None => panic!("collection allocation layout exceeds the platform limit"),
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

impl From<AllocationError> for std::io::Error {
    fn from(_: AllocationError) -> Self {
        std::io::ErrorKind::OutOfMemory.into()
    }
}

pub(crate) fn try_box_uninit<T>(len: usize) -> Result<Box<[mem::MaybeUninit<T>]>, AllocationError> {
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

/// Allocates and initializes one exact boxed slice without invoking the global
/// allocation-error handler.
pub fn try_box_with<T>(
    len: usize,
    mut initialize: impl FnMut(usize) -> T,
) -> Result<Box<[T]>, AllocationError> {
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

    let mut entries = try_box_uninit(len)?;
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

/// Allocates one boxed value without invoking the global allocation-error handler.
pub fn try_box<T>(value: T) -> Result<Box<T>, AllocationError> {
    let mut value = Some(value);
    let mut entries = try_box_with(1, |_| {
        value.take().expect("single boxed value initialized twice")
    })?;
    let entry = entries.as_mut_ptr();
    mem::forget(entries);
    // SAFETY: the one-element boxed slice owns exactly this initialized value.
    Ok(unsafe { Box::from_raw(entry) })
}

/// Reserves an exact vector capacity without invoking the global allocation-error handler.
pub fn try_vec_with_capacity<T>(capacity: usize) -> Result<Vec<T>, AllocationError> {
    let layout = alloc::Layout::array::<T>(capacity).map_err(|_| AllocationError::overflow())?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| AllocationError::exhausted(layout))?;
    Ok(values)
}

pub(super) struct BoxSliceGrowth<'a, T> {
    target: &'a mut Box<[T]>,
    values: Vec<T>,
}

impl<'a, T> BoxSliceGrowth<'a, T> {
    pub(super) fn take(target: &'a mut Box<[T]>) -> Self {
        let values = mem::take(target).into_vec();
        Self { target, values }
    }
}

impl<T> ops::Deref for BoxSliceGrowth<'_, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<T> ops::DerefMut for BoxSliceGrowth<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

impl<T> Drop for BoxSliceGrowth<'_, T> {
    fn drop(&mut self) {
        *self.target = mem::take(&mut self.values).into_boxed_slice();
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
