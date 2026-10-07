use std::{alloc, marker, mem, num::NonZero, ptr};

pub struct Layout<T>(pub(in crate::alloc::raw) marker::PhantomData<fn() -> T>);

impl<T> Layout<T> {
    /// Largest unpadded byte extent accepted at this element alignment.
    pub(crate) const MAX: u128 = isize::MAX as u128 - (mem::align_of::<T>() as u128 - 1);

    pub(crate) const fn fits(len: usize) -> bool {
        let size = mem::size_of::<T>() as u128;
        size == 0 || len as u128 <= Self::MAX / size
    }

    pub(crate) const fn try_check(len: usize) -> Result<(), crate::alloc::Overflow> {
        if !Self::fits(len) {
            Err(crate::alloc::Overflow)
        } else {
            Ok(())
        }
    }

    pub(crate) const fn try_layout(len: usize) -> Result<alloc::Layout, crate::alloc::Overflow> {
        if let Err(error) = Self::try_check(len) {
            return Err(error);
        }
        // SAFETY: admission proves the complete array at its element alignment.
        Ok(unsafe { Self::layout(len) })
    }

    pub(crate) const fn try_extend(
        prior: alloc::Layout,
        len: usize,
    ) -> Result<(alloc::Layout, usize), crate::alloc::Overflow> {
        let bounds = Bounds::new(prior).extend::<T>(len);
        if !bounds.fits() {
            return Err(crate::alloc::Overflow);
        }
        // SAFETY: fits admits this complete extent at its retained alignment.
        Ok((unsafe { bounds.layout() }, bounds.offset))
    }

    pub(crate) const fn fits_after(prior: alloc::Layout, len: usize) -> bool {
        Bounds::new(prior).extend::<T>(len).fits()
    }

    /// # Safety
    /// This exact element count forms a representable array layout.
    pub(crate) const unsafe fn layout(len: usize) -> alloc::Layout {
        // SAFETY: the caller proves this exact array's size and alignment.
        unsafe {
            Self::at(
                0,
                len,
                NonZero::<usize>::MIN.saturating_add(mem::align_of::<T>() - 1),
            )
        }
    }

    /// # Safety
    /// Rounding `size` to this element alignment fits an allocation layout.
    pub(crate) const unsafe fn offset(size: usize) -> usize {
        // SAFETY: the caller's layout proof includes this aligned boundary.
        unsafe { alloc::Layout::from_size_align_unchecked(size, mem::align_of::<T>()) }
            .pad_to_align()
            .size()
    }

    /// # Safety
    /// Extending `prior` by this exact array forms a representable layout.
    pub(crate) const unsafe fn extend(prior: alloc::Layout, len: usize) -> (alloc::Layout, usize) {
        // SAFETY: the extension proof covers the aligned offset and complete size.
        let offset = unsafe { Self::offset(prior.size()) };
        let align = NonZero::<usize>::MIN.saturating_add(prior.align() - 1);
        (unsafe { Self::at(offset, len, align) }, offset)
    }

    /// # Safety
    /// `offset` is aligned for `T`; the array starting there fits a layout at the larger of `align` and `T`'s alignment.
    /// `align` is a power of two; that complete extent bounds the product and sum.
    pub(crate) const unsafe fn at(
        offset: usize,
        len: usize,
        align: NonZero<usize>,
    ) -> alloc::Layout {
        let size = offset + mem::size_of::<T>() * len;
        let element = mem::align_of::<T>();
        let align = if align.get() > element {
            align.get()
        } else {
            element
        };
        // SAFETY: the caller proves the complete size at this combined alignment.
        unsafe { alloc::Layout::from_size_align_unchecked(size, align) }
    }

    pub(in crate::alloc) fn try_grow<const GEOMETRIC: bool>(
        values: &mut Vec<T>,
        extra: usize,
    ) -> Result<(), crate::alloc::Error> {
        let needed = values
            .len()
            .checked_add(extra)
            .ok_or(crate::alloc::Error::Overflow)?;
        if needed <= values.capacity() {
            return Ok(());
        }
        let capacity = if GEOMETRIC {
            Self::reserve(values.capacity(), needed)
        } else {
            needed
        };
        let target = Self::try_layout(capacity)?;
        // SAFETY: a live Vec proves its original allocation layout, including ZSTs.
        let source = unsafe { Self::layout(values.capacity()) };
        // SAFETY: Vec supplies a non-null allocation pointer, including empty
        // capacity, without creating a slice borrow limited to initialized entries.
        let pointer = unsafe { ptr::NonNull::new_unchecked(values.as_mut_ptr().cast::<u8>()) };
        // SAFETY: needed exceeds Vec capacity; the zero-sized source uses the
        // checked allocator entry point, while nonzero growth retains alignment
        // and ownership of the source.
        let pointer = unsafe {
            if source.size() == 0 {
                crate::alloc::raw::LayoutExt::try_allocate(target)
            } else {
                crate::alloc::raw::resize(source, pointer, target)
            }
        }?;
        let len = values.len();
        // SAFETY: successful growth consumed the old allocation. Replace its stale
        // representation without dropping the retained initialized elements.
        unsafe {
            ptr::write(
                values,
                Vec::from_raw_parts(pointer.cast::<T>().as_ptr(), len, capacity),
            )
        };
        Ok(())
    }

    pub(crate) fn reserve(capacity: usize, required: usize) -> usize {
        if required <= capacity {
            return capacity;
        }
        let minimum = match mem::size_of::<T>() {
            1 => 8,
            0..=1024 => 4,
            _ => 1,
        };
        required.max(capacity.saturating_mul(2)).max(minimum)
    }
}

/// Numeric layout extent; saturation preserves rejection above the isize domain.
#[derive(Debug)]
pub(crate) struct Bounds {
    size: usize,
    align: NonZero<usize>,
    offset: usize,
}

impl Bounds {
    pub(crate) const fn new(layout: alloc::Layout) -> Self {
        Self {
            size: layout.size(),
            align: NonZero::<usize>::MIN.saturating_add(layout.align() - 1),
            offset: 0,
        }
    }

    pub(crate) const fn extend<T>(mut self, len: usize) -> Self {
        let element = mem::align_of::<T>();
        let align = if self.align.get() > element {
            self.align.get()
        } else {
            element
        };
        let offset = self.size.saturating_add(element - 1) & !(element - 1);
        self.size = offset.saturating_add(mem::size_of::<T>().saturating_mul(len));
        self.align = NonZero::<usize>::MIN.saturating_add(align - 1);
        self.offset = offset;
        self
    }

    pub(crate) const fn fits(&self) -> bool {
        self.size <= isize::MAX as usize - (self.align.get() - 1)
    }

    /// # Safety
    /// fits has admitted this exact numeric extent and retained alignment.
    pub(crate) const unsafe fn layout(&self) -> alloc::Layout {
        // SAFETY: the caller retains this exact extent's representability proof.
        unsafe { alloc::Layout::from_size_align_unchecked(self.size, self.align.get()) }
    }
}

/// One array recipe awaiting checked admission or a retained layout proof.
pub(crate) struct Root<T> {
    len: usize,
    value: marker::PhantomData<fn() -> T>,
}

impl<T> Root<T> {
    pub(crate) const fn new(len: usize) -> Self {
        Self {
            len,
            value: marker::PhantomData,
        }
    }
}

pub(super) struct Tail<R, T> {
    pub(super) prior: R,
    pub(super) len: usize,
    pub(super) value: marker::PhantomData<fn() -> T>,
}

impl<T> crate::alloc::raw::Recipe for Root<T> {
    type Fields = crate::alloc::raw::Layout<T>;

    fn try_build(self) -> Result<crate::alloc::raw::Bundle<Self::Fields>, crate::alloc::Overflow> {
        crate::alloc::raw::Bundle::try_root::<T>(self.len)
    }

    unsafe fn build(self) -> crate::alloc::raw::Bundle<Self::Fields> {
        unsafe { crate::alloc::raw::Bundle::root::<T>(self.len) }
    }
}

impl<R: crate::alloc::raw::Recipe, T> crate::alloc::raw::Recipe for Tail<R, T> {
    type Fields = crate::alloc::raw::Tail<R::Fields, T>;

    fn try_build(self) -> Result<crate::alloc::raw::Bundle<Self::Fields>, crate::alloc::Overflow> {
        self.prior.try_build()?.try_tail::<T>(self.len)
    }

    unsafe fn build(self) -> crate::alloc::raw::Bundle<Self::Fields> {
        unsafe { self.prior.build().tail::<T>(self.len) }
    }
}
