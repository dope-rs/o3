use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::{Bound, Deref, Range, RangeBounds};
use std::ptr::NonNull;
use std::rc::Rc;
use std::slice::from_raw_parts;

use super::RangeExt;
use super::owned::{Block, Owned};
use super::raw::{Owner, RawSpan};

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

    /// # Panics
    /// Panics if `range` is reversed or out of bounds.
    #[track_caller]
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n.saturating_add(1),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n.saturating_add(1),
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.len,
        };
        let range = start..end;
        assert!(
            range.is_within(self.len),
            "buffer::Shared::slice: range out of bounds"
        );
        if range.is_empty() {
            return Self::new();
        }
        Self {
            ptr: unsafe { self.ptr.add(range.start) },
            len: range.len(),
            owner: self.owner.clone(),
        }
    }

    #[inline]
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

    /// # Panics
    /// Panics if `n` exceeds the remaining length.
    #[inline]
    #[track_caller]
    pub fn advance(&mut self, n: usize) {
        let len = self.len;
        assert!(
            self.try_slice_in_place(n..len),
            "buffer::Shared::advance: out of bounds"
        );
    }

    /// # Panics
    /// Panics if `at` exceeds the remaining length.
    #[track_caller]
    #[must_use]
    pub fn split_to(&mut self, at: usize) -> Self {
        assert!(at <= self.len, "buffer::Shared::split_to: out of bounds");
        if at == 0 {
            return Self::new();
        }
        if at == self.len {
            return mem::take(self);
        }
        let head = Self {
            ptr: self.ptr,
            len: at,
            owner: self.owner.clone(),
        };
        self.ptr = unsafe { self.ptr.add(at) };
        self.len -= at;
        head
    }

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

impl From<Owned> for Shared {
    fn from(value: Owned) -> Self {
        value.freeze()
    }
}

impl From<Block> for Shared {
    fn from(value: Block) -> Self {
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
