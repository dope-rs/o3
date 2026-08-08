use std::{fmt, hash, ops};

use crate::buffer::{
    self, RangeExt as _,
    storage::{self, raw},
};

pub mod strings;

const VEC_ZERO_COPY_MIN: usize = 512;

/// An immutable byte view whose materialized pointer keeps ownership off reads.
#[derive(Clone)]
pub struct Shared {
    ptr: *const u8,
    len: usize,
    owner: raw::Owner,
}

impl Shared {
    #[must_use]
    pub const fn new() -> Self {
        use std::ptr::NonNull;

        Self {
            ptr: NonNull::<u8>::dangling().as_ptr(),
            len: 0,
            owner: raw::Owner::NONE,
        }
    }

    #[must_use]
    pub const fn from_static(s: &'static [u8]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            owner: raw::Owner::NONE,
        }
    }

    pub(in crate::buffer) fn from_span(span: raw::Span) -> Self {
        let (allocation, ptr, len) = span.into_parts();
        Self {
            ptr,
            len,
            owner: raw::Owner::from_allocation(allocation),
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
            owner: raw::Owner::from_vec(buf),
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
