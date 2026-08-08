use std::{
    fmt,
    hash::{Hash, Hasher},
    ops::Range,
};

use crate::buffer::{self, RangeExt as _};

#[doc(hidden)]
pub trait Storage: buffer::Seal {
    fn as_slice(&self) -> &[u8];
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

#[derive(Clone)]
enum RetainedRepr {
    Frozen {
        frozen: buffer::pool::Frozen,
        start: u32,
        len: u32,
    },
    Shared(buffer::storage::shared::Shared),
}

/// Bytes retained beyond their callback through pooled or shared ownership.
#[derive(Clone)]
pub struct Retained {
    repr: RetainedRepr,
}

/// Read-only bytes that can be retained beyond their current callback.
pub trait Retainable: buffer::Seal + Sized {
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
        Self::from(buffer::storage::shared::Shared::copy_from_slice(slice))
    }

    #[must_use]
    pub fn into_shared(self) -> buffer::storage::shared::Shared {
        match self.storage.repr {
            RetainedRepr::Frozen { frozen, start, len } => {
                buffer::storage::shared::Shared::copy_from_slice(
                    &frozen.as_slice()[start as usize..(start + len) as usize],
                )
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
            self.storage.repr = RetainedRepr::Shared(buffer::storage::shared::Shared::new());
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

impl buffer::PrefixConsumer for Bytes<Retained> {
    fn consume_validated_prefix(&mut self, proof: buffer::PrefixProof) {
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
                    self.repr = RetainedRepr::Shared(buffer::storage::shared::Shared::new());
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
    Self: Retainable,
{
    #[must_use]
    pub fn into_retained(self) -> Bytes<Retained> {
        <Self as Retainable>::into_retained(self)
    }
}

impl buffer::Seal for Borrowed<'_> {}

impl Storage for Borrowed<'_> {
    fn as_slice(&self) -> &[u8] {
        self.slice
    }
}

impl buffer::Seal for Retained {}

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

impl buffer::Seal for Bytes<Borrowed<'_>> {}

impl Retainable for Bytes<Borrowed<'_>> {
    fn as_slice(&self) -> &[u8] {
        Bytes::as_slice(self)
    }

    fn into_retained(self) -> Bytes<Retained> {
        Bytes::<Retained>::copy_from_slice(self.as_slice())
    }
}

impl buffer::Seal for Bytes<Retained> {}

impl Retainable for Bytes<Retained> {
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

impl Retainable for buffer::storage::shared::Shared {
    fn as_slice(&self) -> &[u8] {
        buffer::storage::shared::Shared::as_slice(self)
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

impl<S: Storage> buffer::PrefixLength for Bytes<S> {
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

impl From<buffer::storage::shared::Shared> for Bytes<Retained> {
    fn from(value: buffer::storage::shared::Shared) -> Self {
        Self {
            storage: Retained {
                repr: RetainedRepr::Shared(value),
            },
        }
    }
}

impl From<buffer::pool::Frozen> for Bytes<Retained> {
    fn from(value: buffer::pool::Frozen) -> Self {
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
