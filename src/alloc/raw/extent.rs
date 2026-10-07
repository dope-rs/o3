use std::num::NonZero;

/// Exact allocation length retained through deallocation.
/// # Safety
/// A value and all its copies must always report the same construction length.
pub unsafe trait Extent: Copy {
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize;
}

unsafe impl Extent for usize {
    fn len(&self) -> usize {
        *self
    }
}

unsafe impl Extent for NonZero<u32> {
    fn len(&self) -> usize {
        self.get() as usize
    }
}

unsafe impl Extent for NonZero<usize> {
    fn len(&self) -> usize {
        self.get()
    }
}
