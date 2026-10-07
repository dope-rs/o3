use crate::{alloc, base::marker::ThreadBound};

/// One reserved position following this vector's initialized prefix.
#[derive(Debug)]
#[repr(transparent)]
#[must_use = "insert a value or abandon the reserved position"]
pub struct Vacant<'a, T> {
    pub(in crate::alloc) values: &'a mut Vec<T>,
    pub(in crate::alloc) _bound: ThreadBound,
}

impl<'a, T> Vacant<'a, T> {
    /// Reserves before value construction while retaining the exclusive vector.
    pub fn try_new(values: &'a mut Vec<T>) -> Result<Self, alloc::Error> {
        use alloc::raw::VecExt as _;

        alloc::VecExt::try_reserve(values, 1)?;
        // SAFETY: successful reservation admits the exact retained next slot.
        Ok(unsafe { values.vacancy() })
    }

    /// Initializes the admitted position and publishes its new length once.
    pub fn insert(self, value: T) {
        let len = self.values.len();
        // SAFETY: reservation proved len + 1 fits both usize and this vector's
        // capacity. The exclusive loan preserves its allocation and length,
        // including the aligned dangling storage used for zero-sized elements.
        unsafe {
            self.values.as_mut_ptr().add(len).write(value);
            self.values.set_len(len + 1);
        }
    }
}

const _: () = {
    assert!(size_of::<Vacant<'_, ()>>() == size_of::<usize>());
    assert!(size_of::<Option<Vacant<'_, ()>>>() == size_of::<usize>());
    assert!(!std::mem::needs_drop::<Vacant<'_, ()>>());
};
