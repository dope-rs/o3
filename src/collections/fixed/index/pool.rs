use crate::collections::{AllocationError, BoxSliceExt};

const NONE: u32 = u32::MAX;
const ALLOCATED: u32 = u32::MAX - 1;

/// Fixed-capacity allocator for detached dense indices.
///
/// An allocated index does not borrow the pool, allowing the authority that
/// carries it to outlive later allocations from the same pool.
pub struct Pool {
    links: Box<[u32]>,
    free: u32,
    available: u32,
}

impl Pool {
    pub fn try_with_capacity(capacity: u32) -> Result<Self, AllocationError> {
        let links = BoxSliceExt::try_box_with(capacity as usize, |index| {
            let next = index as u32 + 1;
            if next == capacity { NONE } else { next }
        })?;
        Ok(Self {
            links,
            free: if capacity == 0 { NONE } else { 0 },
            available: capacity,
        })
    }

    #[inline]
    pub fn capacity(&self) -> u32 {
        self.links.len() as u32
    }

    #[inline]
    pub fn available(&self) -> u32 {
        self.available
    }

    #[inline]
    pub fn is_exhausted(&self) -> bool {
        self.free == NONE
    }

    /// Detaches one available index from the pool.
    #[inline]
    pub fn take(&mut self) -> Option<u32> {
        if self.is_exhausted() {
            return None;
        }
        // SAFETY: checked immediately above.
        Some(unsafe { self.take_available() })
    }

    /// Detaches an index known to be available.
    ///
    /// # Safety
    ///
    /// The pool must not be exhausted.
    #[inline]
    pub unsafe fn take_available(&mut self) -> u32 {
        let index = self.free;
        debug_assert_ne!(index, NONE);
        debug_assert!((index as usize) < self.links.len());
        // SAFETY: the precondition proves `free` is an index originating in
        // `links`, and all subsequent heads are read from those links.
        let entry = unsafe { self.links.get_unchecked_mut(index as usize) };
        self.free = *entry;
        *entry = ALLOCATED;
        self.available -= 1;
        index
    }

    /// Returns a detached index to this pool.
    ///
    /// # Safety
    ///
    /// `index` must have been returned by [`Pool::take`] or
    /// [`Pool::take_available`] on this exact pool and must not have been
    /// returned since.
    #[inline]
    pub unsafe fn release(&mut self, index: u32) {
        debug_assert!((index as usize) < self.links.len());
        // SAFETY: required by this function's contract.
        let entry = unsafe { self.links.get_unchecked_mut(index as usize) };
        debug_assert_eq!(*entry, ALLOCATED);
        *entry = self.free;
        self.free = index;
        self.available += 1;
    }
}
