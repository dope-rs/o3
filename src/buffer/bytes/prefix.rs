use std::array;

use crate::{
    base::num::bounded::U32,
    buffer::{bytes::bounded, storage::inline, write::consume},
};

/// A borrowed byte prefix whose length is at most `MAX`.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
#[repr(transparent)]
pub struct Prefix<'a, const MAX: usize>(&'a [u8]);

impl<'a, const MAX: usize> Prefix<'a, MAX> {
    /// Retains the original source and its already established extent bound.
    pub const fn from_raw(proof: bounded::raw::Input<&'a [u8], MAX>) -> Self {
        Self(proof.into_slice())
    }

    /// Preserves the prefix's static length bound without checking its value.
    pub const fn len<const END: u32>(&self) -> U32<0, END> {
        bounded::raw::Input::prefix(self)
    }

    pub fn try_new(bytes: &'a [u8]) -> Result<Self, consume::Error> {
        consume::Error::check(bytes.len(), MAX)?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self(&bytes[..bytes.len().min(MAX)])
    }

    #[must_use]
    pub const fn as_slice(&self) -> &'a [u8] {
        self.0
    }

    #[must_use]
    pub const fn into_slice(self) -> &'a [u8] {
        self.0
    }

    pub fn split_first(self) -> Option<(&'a u8, Self)> {
        let (head, tail) = self.0.split_first()?;
        Some((head, Self(tail)))
    }

    pub fn split_array<const N: usize>(self) -> Option<(&'a [u8; N], Self)> {
        let (head, tail) = self.0.split_first_chunk::<N>()?;
        Some((head, Self(tail)))
    }

    /// Splits at an admitted offset, returning `None` when it exceeds this prefix.
    pub fn split_at(self, mid: usize) -> Option<(Self, Self)> {
        let (head, tail) = self.0.split_at_checked(mid)?;
        Some((Self(head), Self(tail)))
    }
}

impl<'a, const MAX: usize> From<&'a [u8; MAX]> for Prefix<'a, MAX> {
    fn from(bytes: &'a [u8; MAX]) -> Self {
        Self(bytes)
    }
}

impl<'a, const MAX: usize> From<&'a array::IntoIter<u8, MAX>> for Prefix<'a, MAX> {
    fn from(bytes: &'a array::IntoIter<u8, MAX>) -> Self {
        Self(bytes.as_slice())
    }
}

impl<'a, const MAX: usize> From<&'a inline::Bytes<MAX>> for Prefix<'a, MAX> {
    fn from(bytes: &'a inline::Bytes<MAX>) -> Self {
        Self(bytes.as_ref())
    }
}

const _: () = {
    assert!(size_of::<Prefix<'static, 1>>() == size_of::<&'static [u8]>());
    assert!(align_of::<Prefix<'static, 1>>() == align_of::<&'static [u8]>());
};
