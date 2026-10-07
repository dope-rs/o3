use std::num::NonZero;

use crate::depot::queue::links;

unsafe impl crate::depot::raw::Word for u8 {
    const ZERO: Self = 0;
    const MAX: NonZero<usize> = NonZero::<usize>::MIN.saturating_add(u8::MAX as usize - 1);

    fn offset(self) -> usize {
        self as usize
    }

    unsafe fn from_offset(offset: usize) -> Self {
        offset as Self
    }
}

unsafe impl crate::depot::raw::Word for u16 {
    const ZERO: Self = 0;
    const MAX: NonZero<usize> = NonZero::<usize>::MIN.saturating_add(u16::MAX as usize - 1);

    fn offset(self) -> usize {
        self as usize
    }

    unsafe fn from_offset(offset: usize) -> Self {
        offset as Self
    }
}

unsafe impl crate::depot::raw::Word for u32 {
    const ZERO: Self = 0;
    const MAX: NonZero<usize> = NonZero::<usize>::MIN.saturating_add(u32::MAX as usize - 1);

    fn offset(self) -> usize {
        self as usize
    }

    unsafe fn from_offset(offset: usize) -> Self {
        offset as Self
    }
}

unsafe impl crate::depot::raw::Word for usize {
    const ZERO: Self = 0;
    const MAX: NonZero<usize> = NonZero::<usize>::MAX;

    fn offset(self) -> usize {
        self
    }

    unsafe fn from_offset(offset: usize) -> Self {
        offset
    }
}

unsafe impl crate::depot::raw::Word for links::Index {
    const ZERO: Self = unsafe { Self::new_raw(0) };
    const MAX: NonZero<usize> = NonZero::<usize>::MIN.saturating_add(u32::MAX as usize - 2);

    fn offset(self) -> usize {
        self.raw() as usize
    }

    unsafe fn from_offset(offset: usize) -> Self {
        // SAFETY: Word bounds the offset below the sole compact sentinel.
        unsafe { Self::new_usize(offset) }
    }
}
