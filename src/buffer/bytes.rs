use std::{
    convert::Infallible,
    fmt,
    hash::{Hash, Hasher},
    ops::Range,
};

use crate::buffer::{
    CapacityError, Frozen, PrefixConsumer, PrefixLength, PrefixProof, RangeExt, SpareWriter,
    checked_append_len, queue::Ring, shared::Shared, write_vec_slices,
};

pub trait Storage {
    fn as_slice(&self) -> &[u8];
}

pub trait RetainBytesSealed {}

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

#[derive(Clone)]
enum RetainedRepr {
    Frozen {
        frozen: Frozen,
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

/// Read-only bytes that can be retained beyond their current callback.
pub trait RetainBytes: RetainBytesSealed + Sized {
    fn as_slice(&self) -> &[u8];

    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Promotes borrowed bytes by copying and owned bytes by transferring ownership.
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

impl Bytes<Retained> {
    #[must_use]
    pub fn copy_from_slice(slice: &[u8]) -> Self {
        Self::from(Shared::copy_from_slice(slice))
    }

    #[must_use]
    pub fn into_shared(self) -> Shared {
        match self.storage.repr {
            RetainedRepr::Frozen { frozen, start, len } => {
                Shared::copy_from_slice(&frozen.as_slice()[start as usize..(start + len) as usize])
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
            RetainedRepr::Frozen { start, len, .. } => {
                *start += amount as u32;
                *len -= amount as u32;
            }
            RetainedRepr::Shared(shared) => shared.consume_valid(amount),
        }
    }
}

impl PrefixConsumer for Bytes<Retained> {
    fn consume_validated_prefix(&mut self, proof: PrefixProof) {
        self.consume_valid(proof.amount());
    }
}

impl Retained {
    fn len(&self) -> usize {
        match &self.repr {
            RetainedRepr::Frozen { len, .. } => *len as usize,
            RetainedRepr::Shared(shared) => shared.len(),
        }
    }

    fn try_slice_in_place(&mut self, range: Range<usize>) -> bool {
        match &mut self.repr {
            RetainedRepr::Frozen { start, len, .. } => {
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

impl<S: Storage> Bytes<S> {
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

impl Storage for Borrowed<'_> {
    fn as_slice(&self) -> &[u8] {
        self.slice
    }
}

impl Storage for Retained {
    fn as_slice(&self) -> &[u8] {
        match &self.repr {
            RetainedRepr::Frozen { frozen, start, len } => {
                &frozen.as_slice()[*start as usize..(*start + *len) as usize]
            }
            RetainedRepr::Shared(shared) => shared.as_slice(),
        }
    }
}

impl RetainBytesSealed for Bytes<Borrowed<'_>> {}

impl RetainBytes for Bytes<Borrowed<'_>> {
    fn as_slice(&self) -> &[u8] {
        Bytes::as_slice(self)
    }

    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::copy_from_slice(self.as_slice())
    }
}

impl RetainBytesSealed for Bytes<Retained> {}

impl RetainBytes for Bytes<Retained> {
    fn as_slice(&self) -> &[u8] {
        Bytes::as_slice(self)
    }

    fn is_empty(&self) -> bool {
        match &self.storage.repr {
            RetainedRepr::Frozen { frozen, len, .. } => frozen.is_empty() || *len == 0,
            RetainedRepr::Shared(shared) => shared.is_empty(),
        }
    }

    fn into_retained(self) -> Bytes<Retained> {
        self
    }
}

impl RetainBytesSealed for Shared {}

impl RetainBytes for Shared {
    fn as_slice(&self) -> &[u8] {
        Shared::as_slice(self)
    }

    fn into_retained(self) -> Bytes<Retained> {
        Bytes {
            storage: Retained {
                repr: RetainedRepr::Shared(self),
            },
        }
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

impl<S: Storage> AsRef<[u8]> for Bytes<S> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<S: Storage> PrefixLength for Bytes<S> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<S: Storage> PartialEq for Bytes<S> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<S: Storage> Eq for Bytes<S> {}

impl<S: Storage> Hash for Bytes<S> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<S: Storage> fmt::Debug for Bytes<S> {
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

impl From<Shared> for Bytes<Retained> {
    fn from(value: Shared) -> Self {
        Self {
            storage: Retained {
                repr: RetainedRepr::Shared(value),
            },
        }
    }
}

impl From<Frozen> for Bytes<Retained> {
    fn from(value: Frozen) -> Self {
        let len = value.len() as u32;
        Self {
            storage: Retained {
                repr: RetainedRepr::Frozen {
                    frozen: value,
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

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error>;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error>;
}

impl ByteSink for Vec<u8> {
    type Error = Infallible;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.push(byte);
        Ok(())
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        write_vec_slices(self, slices);
        Ok(())
    }
}

impl ByteSink for SpareWriter<'_> {
    type Error = CapacityError;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.try_push(byte)
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.try_extend(bytes)
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_extend_from_slices(slices)
    }
}

impl ByteSink for Ring {
    type Error = CapacityError;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.try_push(byte)
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.try_extend(bytes)
    }

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

    pub fn finish(self) -> usize {
        self.written
    }
}

impl ByteSink for SliceWriter<'_> {
    type Error = CapacityError;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        if self.written == self.out.len() {
            return Err(CapacityError::new(
                self.written.saturating_add(1),
                self.out.len(),
            ));
        }
        self.out[self.written] = byte;
        self.written += 1;
        Ok(())
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let end = self
            .written
            .checked_add(bytes.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, self.out.len()))?;
        if end > self.out.len() {
            return Err(CapacityError::new(end, self.out.len()));
        }
        self.out[self.written..end].copy_from_slice(bytes);
        self.written = end;
        Ok(())
    }

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
