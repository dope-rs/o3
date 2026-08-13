use crate::collections;

const NONE: u32 = u32::MAX;
const ALLOCATED: u32 = u32::MAX - 1;

/// Fixed-capacity allocator for detached dense indices.
pub struct Pool {
    links: Box<[u32]>,
    free: u32,
    available: u32,
}

impl Pool {
    pub fn try_with_capacity(capacity: u32) -> Result<Self, collections::AllocationError> {
        let links = collections::BoxSliceExt::try_box_with(capacity as usize, |index| {
            let next = index as u32 + 1;
            if next == capacity { NONE } else { next }
        })?;
        Ok(Self {
            links,
            free: if capacity == 0 { NONE } else { 0 },
            available: capacity,
        })
    }

    pub fn capacity(&self) -> u32 {
        self.links.len() as u32
    }

    pub fn available(&self) -> u32 {
        self.available
    }

    pub fn is_exhausted(&self) -> bool {
        self.free == NONE
    }

    /// Detaches one available index from the pool.
    pub fn take(&mut self) -> Option<u32> {
        if self.is_exhausted() {
            return None;
        }
        let index = self.free;
        let entry = self.links.get_mut(index as usize)?;
        self.free = *entry;
        *entry = ALLOCATED;
        self.available -= 1;
        Some(index)
    }

    /// Returns an allocated index, or rejects an invalid/double release.
    pub fn release(&mut self, index: u32) -> bool {
        let Some(entry) = self.links.get_mut(index as usize) else {
            return false;
        };
        if *entry != ALLOCATED {
            return false;
        }
        *entry = self.free;
        self.free = index;
        self.available += 1;
        true
    }
}
