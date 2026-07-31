use std::convert::Infallible;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::slice;

use super::pool::shared::Pooled;
use super::prefix::consume_prefix_api;
use super::shared::Shared;
use super::{
    ByteRing, CapacityError, PrefixLength, RangeExt, SpareWriter, checked_append_len,
    write_vec_slices,
};

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

    fn len(&self) -> usize {
        self.as_slice().len()
    }

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
    #[must_use]
    pub fn get(self, range: Range<usize>) -> Option<Self> {
        if !range.is_within(self.storage.slice.len()) {
            return None;
        }
        Some(Self {
            storage: Borrowed {
                slice: &self.storage.slice[range],
            },
        })
    }
}

impl Bytes<Shared> {
    #[must_use]
    pub fn into_shared(self) -> Shared {
        self.storage
    }

    #[must_use]
    pub fn get(mut self, range: Range<usize>) -> Option<Self> {
        self.storage.try_slice_in_place(range).then_some(self)
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

    #[must_use]
    pub fn get(mut self, range: Range<usize>) -> Option<Self> {
        self.storage.try_slice_in_place(range).then_some(self)
    }

    pub fn try_advance(&mut self, n: usize) -> bool {
        let len = self.storage.len();
        self.storage.try_slice_in_place(n..len)
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        if amount == self.storage.len() {
            self.storage.repr = RetainedRepr::Shared(Shared::new());
            return;
        }
        match &mut self.storage.repr {
            RetainedRepr::Leased { start, len, .. } => {
                *start += amount as u32;
                *len -= amount as u32;
            }
            RetainedRepr::Shared(shared) => shared.consume_valid(amount),
        }
    }

    consume_prefix_api!(Self::consume_valid);
}

impl Retained {
    fn len(&self) -> usize {
        match &self.repr {
            RetainedRepr::Leased { len, .. } => *len as usize,
            RetainedRepr::Shared(shared) => shared.len(),
        }
    }

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
    pub fn as_slice(&self) -> &[u8] {
        self.storage.as_slice()
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

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
    fn as_slice(&self) -> &[u8] {
        self.slice
    }
}

impl sealed::Storage for Leased {
    fn as_slice(&self) -> &[u8] {
        self.pooled.as_slice()
    }
}

impl sealed::Storage for Shared {
    fn as_slice(&self) -> &[u8] {
        let slice = Shared::as_slice(self);
        debug_assert_eq!(slice.len(), self.len());
        slice
    }
}

impl sealed::Storage for Retained {
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
    fn as_slice(&self) -> &[u8] {
        let slice = self.storage.as_slice();
        debug_assert_eq!(slice.len(), self.len());
        slice
    }
}

impl sealed::RetainBytes for Bytes<Borrowed<'_>> {}

impl RetainBytes for Bytes<Borrowed<'_>> {
    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::copy_from_slice(self.as_slice())
    }
}

impl sealed::RetainBytes for Bytes<Leased> {}

impl RetainBytes for Bytes<Leased> {
    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::from(self.storage.pooled)
    }
}

impl sealed::RetainBytes for Bytes<Shared> {}

impl RetainBytes for Bytes<Shared> {
    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::from(self.storage)
    }
}

impl sealed::RetainBytes for Bytes<Retained> {}

impl RetainBytes for Bytes<Retained> {
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

impl<S: sealed::Storage> PrefixLength for Bytes<S> {
    fn prefix_len(&self) -> usize {
        self.len()
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

/// A byte destination whose individual writes either append completely or
/// leave its logical output unchanged.
pub trait ByteSink {
    type Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error>;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.write_slices([bytes])
    }

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.write_slices([slice::from_ref(&byte)])
    }
}

impl ByteSink for Vec<u8> {
    type Error = Infallible;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        write_vec_slices(self, slices);
        Ok(())
    }
}

impl ByteSink for SpareWriter<'_> {
    type Error = CapacityError;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_extend_from_slices(slices)
    }
}

impl ByteSink for ByteRing {
    type Error = CapacityError;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_extend_from_slices(slices)
    }
}

/// A checked cursor over an initialized output slice.
pub struct SliceWriter<'a> {
    out: &'a mut [u8],
    written: usize,
}

impl<'a> SliceWriter<'a> {
    pub fn new(out: &'a mut [u8]) -> Self {
        Self { out, written: 0 }
    }

    pub const fn len(&self) -> usize {
        self.written
    }

    pub const fn is_empty(&self) -> bool {
        self.written == 0
    }

    pub const fn remaining(&self) -> usize {
        self.out.len() - self.written
    }

    pub fn finish(self) -> usize {
        self.written
    }
}

impl ByteSink for SliceWriter<'_> {
    type Error = CapacityError;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        let end = checked_append_len(self.written, self.out.len(), &slices)?;
        let mut offset = self.written;
        for src in slices {
            let next = offset + src.len();
            self.out[offset..next].copy_from_slice(src);
            offset = next;
        }
        self.written = end;
        Ok(())
    }
}
