use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

use super::storage::{RawMut, RawSpan};
use super::{CapacityError, ExactBuildError, PrefixLength, SpareWriter};
use crate::buffer::Shared;

pub const BLOCK_CAPACITY: u32 = 64 * 1024;

/// A uniquely owned, non-growing byte allocation. `CAP == 0` selects an exact
/// runtime capacity; a nonzero `CAP` fixes it in the type without added storage.
pub struct Owned<const CAP: u32 = 0> {
    raw: RawMut,
    len: u32,
}

impl Owned {
    pub fn try_with_capacity(capacity: usize) -> Result<Self, CapacityError> {
        let capacity =
            u32::try_from(capacity).map_err(|_| CapacityError::new(capacity, u32::MAX as usize))?;
        Ok(Self::with_capacity_u32(capacity))
    }

    pub fn try_build_exact<E>(
        capacity: usize,
        build: impl FnOnce(&mut SpareWriter<'_>) -> Result<(), E>,
    ) -> Result<Self, ExactBuildError<E>> {
        let mut value = Self::try_with_capacity(capacity).map_err(ExactBuildError::Capacity)?;
        let mut writer = value.spare_writer();
        build(&mut writer).map_err(ExactBuildError::Build)?;
        let actual = writer.finish();
        if actual != capacity {
            return Err(ExactBuildError::LengthMismatch {
                expected: capacity,
                actual,
            });
        }
        Ok(value)
    }

    pub fn try_filled(len: usize, byte: u8) -> Result<Self, CapacityError> {
        let mut value = Self::try_with_capacity(len)?;
        value.raw.fill(byte);
        value.len = len as u32;
        Ok(value)
    }
}

impl<const CAP: u32> Owned<CAP> {
    pub const CAPACITY: usize = CAP as usize;

    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity_u32(CAP)
    }

    fn with_capacity_u32(capacity: u32) -> Self {
        Self {
            raw: RawMut::with_capacity_u32(capacity),
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        if CAP == 0 {
            self.raw.capacity()
        } else {
            CAP as usize
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        self.raw.initialized(self.len())
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.raw.initialized_mut(self.len as usize)
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let start = self.len();
        let capacity = self.capacity();
        let end = start
            .checked_add(src.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(CapacityError::new(end, capacity));
        }
        self.raw.copy_from_slice(start, src);
        self.len = end as u32;
        Ok(())
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), CapacityError> {
        let start = self.len();
        let capacity = self.capacity();
        let mut end = start;
        for slice in &slices {
            end = end
                .checked_add(slice.len())
                .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
            if end > capacity {
                return Err(CapacityError::new(end, capacity));
            }
        }
        let mut offset = start;
        for slice in slices {
            self.raw.copy_from_slice(offset, slice);
            offset += slice.len();
        }
        self.len = end as u32;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        let offset = self.len();
        let capacity = self.capacity();
        if offset == capacity {
            return Err(CapacityError::new(offset.saturating_add(1), capacity));
        }
        self.raw.write_byte(offset, byte);
        self.len += 1;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        self.raw.spare_writer(&mut self.len)
    }

    #[must_use]
    pub fn freeze(self) -> Shared {
        let Self { raw, len } = self;
        if len == 0 {
            return Shared::new();
        }
        // SAFETY: every constructor and writer maintains `len <= raw.capacity()`.
        let span = unsafe { RawSpan::new_unchecked(raw.freeze(), 0, len) };
        Shared::from_raw_span(span)
    }
}

impl<const CAP: u32> Default for Owned<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: u32> Clone for Owned<CAP> {
    fn clone(&self) -> Self {
        let mut clone = Self::with_capacity_u32(self.raw.capacity() as u32);
        if self.len != 0 {
            clone.raw.copy_from_raw(0, &self.raw, 0, self.len());
            clone.len = self.len;
        }
        clone
    }
}

impl<const CAP: u32> AsRef<[u8]> for Owned<CAP> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const CAP: u32> PrefixLength for Owned<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<const CAP: u32> AsMut<[u8]> for Owned<CAP> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl<const CAP: u32> Deref for Owned<CAP> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const CAP: u32> DerefMut for Owned<CAP> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<const CAP: u32, const OTHER: u32> PartialEq<Owned<OTHER>> for Owned<CAP> {
    fn eq(&self, other: &Owned<OTHER>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<const CAP: u32> Eq for Owned<CAP> {}

impl<const CAP: u32> Hash for Owned<CAP> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<const CAP: u32> fmt::Debug for Owned<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Owned")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}
