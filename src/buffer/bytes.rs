use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use super::{Pooled, RangeExt, Shared};

pub(super) mod sealed {
    pub trait Storage {
        fn as_slice(&self) -> &[u8];
    }

    pub trait ByteSpan {}
    pub trait RetainBytes {}
}

/// Bytes with a statically selected storage policy and no wrapper overhead.
#[repr(transparent)]
pub struct Bytes<S> {
    storage: S,
}

/// A non-owning byte slice valid for `'a`.
#[derive(Clone, Copy, Default)]
#[repr(transparent)]
pub struct Borrowed<'a> {
    slice: &'a [u8],
}

/// Immutable bytes holding one pooled slot until they are consumed or retained.
#[repr(transparent)]
pub struct Leased {
    pooled: Pooled,
}

#[derive(Clone)]
enum RetainedRepr {
    Leased {
        pooled: Pooled,
        start: u32,
        len: u32,
    },
    Shared(Shared),
}

/// Bytes retained beyond their callback through pooled or shared ownership.
#[derive(Clone)]
pub struct Retained {
    repr: RetainedRepr,
}

/// Read-only access shared by all byte storage policies.
pub trait ByteSpan: sealed::ByteSpan {
    fn as_slice(&self) -> &[u8];

    #[inline]
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

/// Promotes borrowed bytes by copying and owned bytes by transferring ownership.
pub trait RetainBytes: ByteSpan + sealed::RetainBytes + Sized {
    #[must_use]
    fn into_retained(self) -> Bytes<Retained>;
}

impl<'a> Bytes<Borrowed<'a>> {
    /// # Panics
    /// Panics if `range` is reversed or out of bounds.
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn slice(self, range: Range<usize>) -> Self {
        Self {
            storage: Borrowed {
                slice: &self.storage.slice[range],
            },
        }
    }
}

impl Bytes<Shared> {
    #[must_use]
    pub fn into_shared(self) -> Shared {
        self.storage
    }

    /// # Panics
    /// Panics if `range` is reversed or out of bounds.
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn slice(mut self, range: Range<usize>) -> Self {
        assert!(
            self.storage.try_slice_in_place(range),
            "shared byte range is out of bounds"
        );
        self
    }
}

impl Bytes<Retained> {
    #[must_use]
    pub fn copy_from_slice(slice: &[u8]) -> Self {
        Self::from(Shared::copy_from_slice(slice))
    }

    #[must_use]
    pub fn into_shared(self) -> Shared {
        match self.storage.repr {
            RetainedRepr::Leased { pooled, start, len } => {
                Shared::copy_from_slice(&pooled.as_slice()[start as usize..(start + len) as usize])
            }
            RetainedRepr::Shared(shared) => shared,
        }
    }

    /// # Panics
    /// Panics if `range` is reversed or out of bounds.
    #[inline]
    #[track_caller]
    #[must_use]
    pub fn slice(mut self, range: Range<usize>) -> Self {
        assert!(
            self.storage.try_slice_in_place(range),
            "retained byte range is out of bounds"
        );
        self
    }

    /// # Panics
    /// Panics if `n` exceeds the remaining length.
    #[inline]
    #[track_caller]
    pub fn advance(&mut self, n: usize) {
        let len = self.storage.len();
        assert!(
            self.storage.try_slice_in_place(n..len),
            "retained bytes advance is out of bounds"
        );
    }
}

impl Retained {
    fn len(&self) -> usize {
        match &self.repr {
            RetainedRepr::Leased { len, .. } => *len as usize,
            RetainedRepr::Shared(shared) => shared.len(),
        }
    }

    #[inline]
    fn try_slice_in_place(&mut self, range: Range<usize>) -> bool {
        match &mut self.repr {
            RetainedRepr::Leased { start, len, .. } => {
                if !range.is_within(*len as usize) {
                    return false;
                }
                if range.is_empty() {
                    self.repr = RetainedRepr::Shared(Shared::new());
                    return true;
                }
                *start += range.start as u32;
                *len = range.len() as u32;
                true
            }
            RetainedRepr::Shared(shared) => shared.try_slice_in_place(range),
        }
    }
}

impl<S: sealed::Storage> Bytes<S> {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.storage.as_slice()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

impl<S> Bytes<S>
where
    Self: RetainBytes,
{
    #[must_use]
    pub fn into_retained(self) -> Bytes<Retained> {
        <Self as RetainBytes>::into_retained(self)
    }
}

impl sealed::Storage for Borrowed<'_> {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        self.slice
    }
}

impl sealed::Storage for Leased {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        self.pooled.as_slice()
    }
}

impl sealed::Storage for Shared {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        Shared::as_slice(self)
    }
}

impl sealed::Storage for Retained {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        match &self.repr {
            RetainedRepr::Leased { pooled, start, len } => {
                &pooled.as_slice()[*start as usize..(*start + *len) as usize]
            }
            RetainedRepr::Shared(shared) => shared.as_slice(),
        }
    }
}

impl<S: sealed::Storage> sealed::ByteSpan for Bytes<S> {}

impl<S: sealed::Storage> ByteSpan for Bytes<S> {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        Self::as_slice(self)
    }
}

impl sealed::RetainBytes for Bytes<Borrowed<'_>> {}

impl RetainBytes for Bytes<Borrowed<'_>> {
    #[inline]
    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::copy_from_slice(self.as_slice())
    }
}

impl sealed::RetainBytes for Bytes<Leased> {}

impl RetainBytes for Bytes<Leased> {
    #[inline]
    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::from(self.storage.pooled)
    }
}

impl sealed::RetainBytes for Bytes<Shared> {}

impl RetainBytes for Bytes<Shared> {
    #[inline]
    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::from(self.storage)
    }
}

impl sealed::RetainBytes for Bytes<Retained> {}

impl RetainBytes for Bytes<Retained> {
    #[inline]
    fn into_retained(self) -> Bytes<Retained> {
        self
    }
}

impl<S: Clone> Clone for Bytes<S> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
        }
    }
}

impl<S: Copy> Copy for Bytes<S> {}

impl<S: sealed::Storage> AsRef<[u8]> for Bytes<S> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<S: sealed::Storage> PartialEq for Bytes<S> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<S: sealed::Storage> Eq for Bytes<S> {}

impl<S: sealed::Storage> Hash for Bytes<S> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<S: sealed::Storage> fmt::Debug for Bytes<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Bytes").field(&self.as_slice()).finish()
    }
}

impl<'a> From<&'a [u8]> for Bytes<Borrowed<'a>> {
    fn from(value: &'a [u8]) -> Self {
        Self {
            storage: Borrowed { slice: value },
        }
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for Bytes<Borrowed<'a>> {
    fn from(value: &'a [u8; N]) -> Self {
        Self::from(value.as_slice())
    }
}

impl From<Pooled> for Bytes<Leased> {
    fn from(value: Pooled) -> Self {
        Self {
            storage: Leased { pooled: value },
        }
    }
}

impl From<Shared> for Bytes<Shared> {
    fn from(value: Shared) -> Self {
        Self { storage: value }
    }
}

impl From<Shared> for Bytes<Retained> {
    fn from(value: Shared) -> Self {
        Self {
            storage: Retained {
                repr: RetainedRepr::Shared(value),
            },
        }
    }
}

impl From<Pooled> for Bytes<Retained> {
    fn from(value: Pooled) -> Self {
        let len = value.len() as u32;
        Self {
            storage: Retained {
                repr: RetainedRepr::Leased {
                    pooled: value,
                    start: 0,
                    len,
                },
            },
        }
    }
}
