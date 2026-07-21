use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

use super::raw::{RawMut, RawSpan};
use super::shared::Shared;
use super::{CapacityError, SpareWriter};

pub(super) const BLOCK_CAPACITY: u32 = 64 * 1024;

/// A uniquely owned, exact-capacity byte allocation that never grows.
pub struct Owned {
    raw: RawMut,
    len: u32,
}

impl Owned {
    pub fn try_with_capacity(capacity: usize) -> Result<Self, CapacityError> {
        let capacity =
            u32::try_from(capacity).map_err(|_| CapacityError::new(capacity, u32::MAX as usize))?;
        Ok(Self::with_capacity_u32(capacity))
    }

    /// Creates a non-growing allocation with exactly `capacity` bytes.
    ///
    /// # Panics
    /// Panics when `capacity` exceeds the buffer representation limit of
    /// [`u32::MAX`]. Use [`try_with_capacity`](Self::try_with_capacity) when the
    /// size is not already constrained by the caller's protocol.
    #[must_use]
    #[track_caller]
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity > u32::MAX as usize {
            panic!("buffer capacity overflow: {capacity} > {}", u32::MAX);
        }
        Self::with_capacity_u32(capacity as u32)
    }

    #[must_use]
    pub fn with_capacity_u32(capacity: u32) -> Self {
        Self {
            raw: RawMut::with_capacity_u32(capacity),
            len: 0,
        }
    }

    #[must_use]
    pub fn filled(len: usize, byte: u8) -> Self {
        let mut value = Self::with_capacity(len);
        value.raw.fill(byte);
        value.len = len as u32;
        value
    }

    pub fn capacity(&self) -> usize {
        self.raw.capacity()
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
        let end = start + src.len();
        let capacity = self.capacity();
        if end > capacity {
            return Err(CapacityError::new(end, capacity));
        }
        self.raw.copy_from_slice(start, src);
        self.len = end as u32;
        Ok(())
    }

    /// Appends `src` without growing the allocation.
    ///
    /// # Panics
    /// Panics when the configured capacity is insufficient. Use
    /// [`try_extend_from_slice`](Self::try_extend_from_slice) when exhaustion is
    /// an expected outcome.
    #[track_caller]
    pub fn extend_from_slice(&mut self, src: &[u8]) {
        if let Err(error) = self.try_extend_from_slice(src) {
            panic!("{error}");
        }
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        let offset = self.len();
        let capacity = self.capacity();
        if offset == capacity {
            return Err(CapacityError::new(offset + 1, capacity));
        }
        self.raw.write_byte(offset, byte);
        self.len += 1;
        Ok(())
    }

    /// Appends one byte without growing the allocation.
    ///
    /// # Panics
    /// Panics when the configured capacity is full. Use
    /// [`try_push`](Self::try_push) when exhaustion is an expected outcome.
    #[track_caller]
    pub fn push(&mut self, byte: u8) {
        if let Err(error) = self.try_push(byte) {
            panic!("{error}");
        }
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

    #[inline]
    #[must_use]
    pub fn freeze(self) -> Shared {
        let Self { raw, len } = self;
        if len == 0 {
            return Shared::new();
        }
        // SAFETY: `Owned` maintains `len <= raw.capacity()` on every mutation.
        let span = unsafe { RawSpan::new_unchecked(raw.freeze(), 0, len) };
        Shared::from_raw_span(span)
    }
}

impl Clone for Owned {
    fn clone(&self) -> Self {
        let mut clone = Self::with_capacity_u32(self.capacity() as u32);
        if self.len != 0 {
            clone.raw.copy_from_raw(0, &self.raw, 0, self.len());
            clone.len = self.len;
        }
        clone
    }
}

impl AsRef<[u8]> for Owned {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for Owned {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl Deref for Owned {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Owned {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl PartialEq for Owned {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<Block> for Owned {
    fn eq(&self, other: &Block) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Owned {}

impl Hash for Owned {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl fmt::Debug for Owned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Owned")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}

/// A uniquely owned, fixed 64 KiB byte block that never grows.
pub struct Block {
    raw: RawMut,
    len: u32,
}

impl Block {
    pub const CAPACITY: usize = BLOCK_CAPACITY as usize;

    #[must_use]
    pub fn new() -> Self {
        Self {
            raw: RawMut::with_capacity_u32(BLOCK_CAPACITY),
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        Self::CAPACITY
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
        let end = start + src.len();
        if end > Self::CAPACITY {
            return Err(CapacityError::new(end, Self::CAPACITY));
        }
        self.raw.copy_from_slice(start, src);
        self.len = end as u32;
        Ok(())
    }

    /// Appends `src` without growing the block.
    ///
    /// # Panics
    /// Panics when the block is full. Use
    /// [`try_extend_from_slice`](Self::try_extend_from_slice) when exhaustion is
    /// an expected outcome.
    #[track_caller]
    pub fn extend_from_slice(&mut self, src: &[u8]) {
        if let Err(error) = self.try_extend_from_slice(src) {
            panic!("{error}");
        }
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        let offset = self.len();
        if offset == Self::CAPACITY {
            return Err(CapacityError::new(offset + 1, Self::CAPACITY));
        }
        self.raw.write_byte(offset, byte);
        self.len += 1;
        Ok(())
    }

    /// Appends one byte without growing the block.
    ///
    /// # Panics
    /// Panics when the block is full. Use [`try_push`](Self::try_push) when
    /// exhaustion is an expected outcome.
    #[track_caller]
    pub fn push(&mut self, byte: u8) {
        if let Err(error) = self.try_push(byte) {
            panic!("{error}");
        }
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

    #[inline]
    #[must_use]
    pub fn freeze(self) -> Shared {
        let Self { raw, len } = self;
        if len == 0 {
            return Shared::new();
        }
        // SAFETY: `Block` maintains `len <= raw.capacity()` on every mutation.
        let span = unsafe { RawSpan::new_unchecked(raw.freeze(), 0, len) };
        Shared::from_raw_span(span)
    }
}

impl Default for Block {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Block {
    fn clone(&self) -> Self {
        let mut clone = Self::new();
        if self.len != 0 {
            clone.raw.copy_from_raw(0, &self.raw, 0, self.len());
            clone.len = self.len;
        }
        clone
    }
}

impl AsRef<[u8]> for Block {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for Block {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl Deref for Block {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Block {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<Owned> for Block {
    fn eq(&self, other: &Owned) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Block {}

impl Hash for Block {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl fmt::Debug for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Block").field("len", &self.len()).finish()
    }
}
