use std::{fmt, hash, ops};

use crate::buffer::{self, RangeExt as _, storage, write};

/// A uniquely owned, non-growing byte allocation. `CAP == 0` selects an exact
/// runtime capacity; a nonzero `CAP` fixes it in the type without added storage.
pub struct Owned<const CAP: u32 = 0> {
    storage: storage::raw::AllocationMut,
    len: u32,
}

impl Owned {
    pub fn try_with_capacity(capacity: usize) -> Result<Self, buffer::CapacityError> {
        let capacity = u32::try_from(capacity)
            .map_err(|_| buffer::CapacityError::new(capacity, u32::MAX as usize))?;
        Ok(Self::with_capacity_u32(capacity))
    }

    pub fn try_build_exact<E>(
        capacity: usize,
        build: impl FnOnce(&mut write::SpareWriter<'_>) -> Result<(), E>,
    ) -> Result<Self, storage::BuildError<E>> {
        use crate::buffer::storage::BuildError;
        let mut value = Self::try_with_capacity(capacity).map_err(BuildError::Capacity)?;
        let mut writer = value.spare_writer();
        build(&mut writer).map_err(BuildError::Build)?;
        let actual = writer.finish();
        if actual != capacity {
            return Err(BuildError::LengthMismatch {
                expected: capacity,
                actual,
            });
        }
        Ok(value)
    }

    pub fn try_filled(len: usize, byte: u8) -> Result<Self, buffer::CapacityError> {
        let mut value = Self::try_with_capacity(len)?;
        value.storage.fill(byte);
        value.len = len as u32;
        Ok(value)
    }
}

impl<const CAP: u32> Owned<CAP> {
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity_u32(CAP)
    }

    fn with_capacity_u32(capacity: u32) -> Self {
        use crate::buffer::storage::raw::AllocationMut;
        Self {
            storage: AllocationMut::with_capacity_u32(capacity),
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        if CAP == 0 {
            self.storage.capacity()
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
        self.storage.initialized(self.len())
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.storage.initialized_mut(self.len as usize)
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        let start = self.len();
        let capacity = self.capacity();
        let end = start
            .checked_add(src.len())
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(buffer::CapacityError::new(end, capacity));
        }
        self.storage.copy_from_slice(start, src);
        self.len = end as u32;
        Ok(())
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        let start = self.len();
        let capacity = self.capacity();
        let mut end = start;
        for slice in &slices {
            end = end
                .checked_add(slice.len())
                .ok_or_else(|| buffer::CapacityError::new(usize::MAX, capacity))?;
            if end > capacity {
                return Err(buffer::CapacityError::new(end, capacity));
            }
        }
        let mut offset = start;
        for slice in slices {
            self.storage.copy_from_slice(offset, slice);
            offset += slice.len();
        }
        self.len = end as u32;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), buffer::CapacityError> {
        let offset = self.len();
        let capacity = self.capacity();
        if offset == capacity {
            return Err(buffer::CapacityError::new(
                offset.saturating_add(1),
                capacity,
            ));
        }
        self.storage.write_byte(offset, byte);
        self.len += 1;
        Ok(())
    }

    pub fn spare_writer(&mut self) -> write::SpareWriter<'_> {
        self.storage.spare_writer(&mut self.len)
    }

    #[must_use]
    pub fn freeze(self) -> Shared {
        let Self { storage, len } = self;
        if len == 0 {
            return Shared::new();
        }
        // SAFETY: every constructor and writer maintains `len <= storage.capacity()`.
        let span = unsafe {
            use crate::buffer::storage::raw::Span;
            Span::new_unchecked(storage.freeze(), 0, len)
        };
        Shared::from_span(span)
    }
}

impl<const CAP: u32> Default for Owned<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: u32> Clone for Owned<CAP> {
    fn clone(&self) -> Self {
        let mut clone = Self::with_capacity_u32(self.storage.capacity() as u32);
        if self.len != 0 {
            clone
                .storage
                .copy_from_allocation(0, &self.storage, 0, self.len());
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

impl<const CAP: u32> buffer::PrefixLength for Owned<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<const CAP: u32> AsMut<[u8]> for Owned<CAP> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl<const CAP: u32> ops::Deref for Owned<CAP> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const CAP: u32> ops::DerefMut for Owned<CAP> {
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

impl<const CAP: u32> hash::Hash for Owned<CAP> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
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

const VEC_ZERO_COPY_MIN: usize = 512;

/// An immutable byte view whose materialized pointer keeps ownership off reads.
#[derive(Clone)]
pub struct Shared {
    ptr: *const u8,
    len: usize,
    owner: storage::raw::Owner,
}

impl Shared {
    #[must_use]
    pub const fn new() -> Self {
        use std::ptr::NonNull;

        Self {
            ptr: NonNull::<u8>::dangling().as_ptr(),
            len: 0,
            owner: storage::raw::Owner::NONE,
        }
    }

    #[must_use]
    pub const fn from_static(s: &'static [u8]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            owner: storage::raw::Owner::NONE,
        }
    }

    pub(in crate::buffer) fn from_span(span: storage::raw::Span) -> Self {
        let (allocation, ptr, len) = span.into_parts();
        Self {
            ptr,
            len,
            owner: storage::raw::Owner::from_allocation(allocation),
        }
    }

    pub(super) fn from_vec(buf: Vec<u8>) -> Self {
        if buf.is_empty() {
            return Self::new();
        }
        if buf.len() < VEC_ZERO_COPY_MIN {
            return Self::copy_from_slice(&buf);
        }
        Self::from_vec_owner(buf)
    }

    fn from_vec_owner(buf: Vec<u8>) -> Self {
        use std::rc::Rc;

        let buf = Rc::new(buf);
        let ptr = buf.as_ptr();
        let len = buf.len();
        Self {
            ptr,
            len,
            owner: storage::raw::Owner::from_vec(buf),
        }
    }

    #[must_use]
    pub fn copy_from_slice(s: &[u8]) -> Self {
        use crate::buffer::storage::raw::Span;
        if s.is_empty() {
            return Self::new();
        }
        match Span::copy_from_slice(s) {
            Some(span) => Self::from_span(span),
            None => Self::copy_large(s),
        }
    }

    #[cold]
    fn copy_large(s: &[u8]) -> Self {
        Self::from_vec_owner(s.to_vec())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes retained by the complete backing allocation.
    /// Static bytes retain no allocation.
    pub fn resident_bytes(&self) -> usize {
        self.owner.resident_bytes()
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            use std::slice::from_raw_parts;
            from_raw_parts(self.ptr, self.len)
        }
    }

    #[must_use]
    pub fn get(&self, range: impl ops::RangeBounds<usize>) -> Option<Self> {
        use std::ops::Bound;

        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n.checked_add(1)?,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n.checked_add(1)?,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.len,
        };
        let range = start..end;
        if !range.is_within(self.len) {
            return None;
        }
        if range.is_empty() {
            return Some(Self::new());
        }
        Some(Self {
            ptr: unsafe { self.ptr.add(range.start) },
            len: range.len(),
            owner: self.owner.clone(),
        })
    }

    pub(in crate::buffer) fn try_slice_in_place(&mut self, range: ops::Range<usize>) -> bool {
        if !range.is_within(self.len) {
            return false;
        }
        if range.is_empty() {
            self.clear();
            return true;
        }
        self.ptr = unsafe { self.ptr.add(range.start) };
        self.len = range.len();
        true
    }

    pub fn try_advance(&mut self, n: usize) -> bool {
        let len = self.len;
        self.try_slice_in_place(n..len)
    }

    pub(in crate::buffer) fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len);
        if amount == self.len {
            self.clear();
            return;
        }
        self.ptr = unsafe { self.ptr.add(amount) };
        self.len -= amount;
    }
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

impl buffer::Seal for Shared {}

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl buffer::PrefixLength for Shared {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl buffer::PrefixConsumer for Shared {
    fn consume_validated_prefix(&mut self, proof: buffer::PrefixProof) {
        self.consume_valid(proof.amount());
    }
}

impl ops::Deref for Shared {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl From<&'static [u8]> for Shared {
    fn from(value: &'static [u8]) -> Self {
        Self::from_static(value)
    }
}

impl<const N: usize> From<&'static [u8; N]> for Shared {
    fn from(value: &'static [u8; N]) -> Self {
        Self::from_static(value)
    }
}

impl From<Vec<u8>> for Shared {
    fn from(value: Vec<u8>) -> Self {
        Self::from_vec(value)
    }
}

impl<const CAP: u32> From<storage::Owned<CAP>> for Shared {
    fn from(value: storage::Owned<CAP>) -> Self {
        value.freeze()
    }
}

impl From<String> for Shared {
    fn from(value: String) -> Self {
        Self::from_vec(value.into_bytes())
    }
}

impl From<&str> for Shared {
    fn from(value: &str) -> Self {
        Self::copy_from_slice(value.as_bytes())
    }
}

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<[u8]> for Shared {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

impl PartialEq<&[u8]> for Shared {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_slice() == *other
    }
}

impl Eq for Shared {}

impl hash::Hash for Shared {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared").field("len", &self.len()).finish()
    }
}
