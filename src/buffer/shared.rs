use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Bound, Deref, RangeBounds};
use std::rc::Rc;
use std::slice::from_raw_parts;

use super::owned::Owned;
use super::raw::Raw;

#[derive(Clone)]
pub struct Shared {
    repr: SharedRepr,
}

#[derive(Clone)]
enum SharedRepr {
    Static(&'static [u8]),
    Raw {
        buf: Raw,
        start: u32,
        len: u32,
    },
    Vec {
        buf: Rc<Vec<u8>>,
        start: u32,
        len: u32,
    },
}

impl Shared {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            repr: SharedRepr::Static(&[]),
        }
    }

    #[must_use]
    pub const fn from_static(s: &'static [u8]) -> Self {
        Self {
            repr: SharedRepr::Static(s),
        }
    }

    pub(super) fn from_raw_range(buf: Raw, start: u32, len: u32) -> Self {
        assert!(
            start
                .checked_add(len)
                .is_some_and(|end| end as usize <= buf.capacity()),
            "buffer::Shared::from_raw_range: range out of bounds (start={start}, len={len}, capacity={})",
            buf.capacity()
        );
        Self {
            repr: SharedRepr::Raw { buf, start, len },
        }
    }

    pub(super) fn from_vec(buf: Vec<u8>) -> Self {
        if buf.is_empty() {
            return Self::new();
        }
        let len = u32::try_from(buf.len()).expect("buffer capacity overflow");
        Self {
            repr: SharedRepr::Vec {
                buf: Rc::new(buf),
                start: 0,
                len,
            },
        }
    }

    #[must_use]
    pub fn copy_from_slice(s: &[u8]) -> Self {
        if s.is_empty() {
            return Self::new();
        }
        let len = s.len();
        assert!(
            len <= u32::MAX as usize,
            "buffer::Shared: payload too large ({len}, max {})",
            u32::MAX
        );
        Self {
            repr: SharedRepr::Raw {
                buf: Raw::from_slice(s),
                start: 0,
                len: len as u32,
            },
        }
    }

    pub fn len(&self) -> usize {
        match &self.repr {
            SharedRepr::Static(s) => s.len(),
            SharedRepr::Raw { len, .. } | SharedRepr::Vec { len, .. } => *len as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.repr {
            SharedRepr::Static(s) => s,
            SharedRepr::Raw { buf, start, len } => unsafe {
                from_raw_parts(buf.data_ptr().add(*start as usize), *len as usize)
            },
            SharedRepr::Vec { buf, start, len } => &buf[*start as usize..(*start + *len) as usize],
        }
    }

    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let len = self.len();
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n.saturating_add(1),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n.saturating_add(1),
            Bound::Excluded(&n) => n,
            Bound::Unbounded => len,
        };
        assert!(
            start <= end && end <= len,
            "buffer::Shared::slice: range out of bounds"
        );
        if start == end {
            return Self::new();
        }
        match &self.repr {
            SharedRepr::Static(s) => Self::from_static(&s[start..end]),
            SharedRepr::Raw {
                buf, start: cur, ..
            } => Self {
                repr: SharedRepr::Raw {
                    buf: buf.clone(),
                    start: *cur + start as u32,
                    len: (end - start) as u32,
                },
            },
            SharedRepr::Vec {
                buf, start: cur, ..
            } => Self {
                repr: SharedRepr::Vec {
                    buf: Rc::clone(buf),
                    start: *cur + start as u32,
                    len: (end - start) as u32,
                },
            },
        }
    }

    pub fn advance(&mut self, n: usize) {
        let len = self.len();
        assert!(n <= len, "buffer::Shared::advance: out of bounds");
        match &mut self.repr {
            SharedRepr::Static(s) => *s = &s[n..],
            SharedRepr::Raw { start, len, .. } | SharedRepr::Vec { start, len, .. } => {
                *start += n as u32;
                *len -= n as u32;
            }
        }
    }

    #[must_use]
    pub fn split_to(&mut self, at: usize) -> Self {
        let head = self.slice(..at);
        self.advance(at);
        head
    }

    pub fn clear(&mut self) {
        self.repr = SharedRepr::Static(&[]);
    }

    pub fn truncate(&mut self, n: usize) {
        let len = self.len();
        if n >= len {
            return;
        }
        match &mut self.repr {
            SharedRepr::Static(s) => *s = &s[..n],
            SharedRepr::Raw { len, .. } | SharedRepr::Vec { len, .. } => *len = n as u32,
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
