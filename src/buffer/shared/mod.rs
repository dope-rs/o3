use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Bound, Deref, Range, RangeBounds};
use std::ptr::NonNull;
use std::rc::Rc;
use std::slice::from_raw_parts;

pub mod snapshot;
pub mod strings;

use super::owned::Owned;
use super::storage::{Owner, RawSpan};
use super::{PrefixLength, RangeExt};

const VEC_ZERO_COPY_MIN: usize = 512;

/// An immutable byte view whose materialized pointer keeps ownership off reads.
#[derive(Clone)]
pub struct Shared {
    ptr: *const u8,
    len: usize,
    owner: Owner,
}

impl Shared {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ptr: NonNull::<u8>::dangling().as_ptr(),
            len: 0,
            owner: Owner::NONE,
        }
    }

    #[must_use]
    pub const fn from_static(s: &'static [u8]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            owner: Owner::NONE,
        }
    }

    pub(super) fn from_raw_span(span: RawSpan) -> Self {
        let (raw, ptr, len) = span.into_parts();
        Self {
            ptr,
            len,
            owner: Owner::from_raw(raw),
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
        let buf = Rc::new(buf);
        let ptr = buf.as_ptr();
        let len = buf.len();
        Self {
            ptr,
            len,
            owner: Owner::from_vec(buf),
        }
    }

    #[must_use]
    pub fn copy_from_slice(s: &[u8]) -> Self {
        if s.is_empty() {
            return Self::new();
        }
        match RawSpan::copy_from_slice(s) {
            Some(span) => Self::from_raw_span(span),
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
        unsafe { from_raw_parts(self.ptr, self.len) }
    }

    #[must_use]
    pub fn get(&self, range: impl RangeBounds<usize>) -> Option<Self> {
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

    pub(super) fn try_slice_in_place(&mut self, range: Range<usize>) -> bool {
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

    pub(super) fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len);
        if amount == self.len {
            self.clear();
            return;
        }
        self.ptr = unsafe { self.ptr.add(amount) };
        self.len -= amount;
    }

    super::prefix::consume_prefix_api!(Self::consume_valid);

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn truncate(&mut self, n: usize) {
        if n < self.len {
            self.len = n;
        }
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl PrefixLength for Shared {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl Deref for Shared {
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

impl<const CAP: u32> From<Owned<CAP>> for Shared {
    fn from(value: Owned<CAP>) -> Self {
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

impl Hash for Shared {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared").field("len", &self.len()).finish()
    }
}
