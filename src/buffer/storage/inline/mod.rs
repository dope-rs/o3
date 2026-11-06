use std::{hash, mem};

pub use append::Append;

use crate::{
    buffer,
    buffer::{bytes, storage::inline, write, write::consume::Error},
};

pub const CAPACITY: usize = 24;

mod append;

/// A cleared inline owner with a compile-time remaining write bound.
pub struct Fill<'a, const CAP: usize, const LEFT: usize = CAP, L = u16> {
    output: &'a mut [u8],
    len: &'a mut L,
}

impl<'a, const CAP: usize, L: crate::buffer::raw::Length> Fill<'a, CAP, CAP, L> {
    pub fn new(target: &'a mut inline::Bytes<CAP, L>) -> Self {
        target.len = L::ZERO;
        Self {
            output: &mut target.bytes,
            len: &mut target.len,
        }
    }
}

impl<'a, const CAP: usize, const LEFT: usize, L: crate::buffer::raw::Length>
    Fill<'a, CAP, LEFT, L>
{
    pub fn write<const MAX: usize, const NEXT: usize>(
        self,
        source: bytes::Prefix<'_, MAX>,
    ) -> (Fill<'a, CAP, NEXT, L>, &'a mut [u8]) {
        const { assert!(MAX <= LEFT && NEXT == LEFT - MAX) }
        let source = source.into_slice();
        // SAFETY: Prefix bounds input by MAX, and the consuming transition
        // retains at least LEFT bytes of this owner's disjoint remaining storage.
        let (copied, output) = unsafe { self.output.split_at_mut_unchecked(source.len()) };
        inline::Append::new(copied, self.len).write(|output| output.copy_from_slice(source));
        (
            Fill {
                output,
                len: self.len,
            },
            copied,
        )
    }

    /// Writes the final bounded source and releases the remaining output borrow.
    pub fn finish(self, source: bytes::Prefix<'_, LEFT>) {
        let _ = self.write::<LEFT, 0>(source);
    }
}

const _: () = assert!(mem::size_of::<Fill<'_, 284>>() == 3 * mem::size_of::<usize>());

/// Fixed-capacity owned bytes with an integer prefix length.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Bytes<const CAP: usize = { CAPACITY }, L = u8> {
    pub(in crate::buffer::storage::inline) bytes: [u8; CAP],
    pub(in crate::buffer::storage::inline) len: L,
}

impl<const CAP: usize, L: crate::buffer::raw::Length> Bytes<CAP, L> {
    const VALID: () = assert!(CAP <= L::MAX, "inline byte capacity exceeds length field");

    #[must_use]
    pub const fn new() -> Self {
        let () = Self::VALID;
        Self {
            bytes: [0; CAP],
            len: L::ZERO,
        }
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        let mut inline = Self::new();
        write::Sink::write_slice(&mut inline, bytes)?;
        Ok(inline)
    }

    /// Reserves an exact region or returns its precise capacity shortfall.
    pub fn try_reserve(&mut self, amount: usize) -> Result<inline::Append<'_, L>, Error> {
        Error::check(amount, self.available())?;
        Ok(self.fit(amount))
    }

    /// Reserves the largest prefix up to `amount` that currently fits.
    pub fn fit(&mut self, amount: usize) -> inline::Append<'_, L> {
        let start = self.len();
        let end = start + amount.min(CAP - start);
        // SAFETY: the clamped suffix stays within this initialized backing.
        let output = unsafe { self.bytes.get_unchecked_mut(start..end) };
        inline::Append::new(output, &mut self.len)
    }

    /// Replaces the suffix after `start`, admitting every source within `limit`.
    /// Both bounds are clamped to this owner's initialized length and capacity.
    /// Rejection leaves the complete backing and initialized length unchanged.
    pub fn try_replace<const N: usize>(
        &mut self,
        start: usize,
        limit: usize,
        slices: [&[u8]; N],
    ) -> Result<(), Error> {
        let start = start.min(self.len());
        let end = Error::checked_end(start, limit.min(CAP), &slices)?;
        // SAFETY: admission bounds every disjoint source within this owner's
        // backing; only the complete copy publishes the initialized end.
        unsafe {
            buffer::raw::SliceExt::copy_to(&slices, self.bytes.as_mut_ptr().add(start));
        }
        self.len = L::from_usize(end);
        Ok(())
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = L::from_usize(len);
        }
    }

    pub fn clear(&mut self) {
        self.len = L::ZERO;
    }

    pub fn len(&self) -> usize {
        self.len.into_usize()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn available(&self) -> usize {
        CAP - self.len()
    }

    /// Returns the complete initialized backing without changing its prefix.
    pub fn backing_mut(&mut self) -> &mut [u8; CAP] {
        &mut self.bytes
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> Default for Bytes<CAP, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length, const N: usize> From<[u8; N]>
    for Bytes<CAP, L>
{
    fn from(bytes: [u8; N]) -> Self {
        const { assert!(N <= CAP, "inline array must fit its backing capacity") };
        Self::from(bytes::Prefix::<CAP>::new(&bytes))
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> From<bytes::Prefix<'_, CAP>>
    for Bytes<CAP, L>
{
    fn from(source: bytes::Prefix<'_, CAP>) -> Self {
        let mut prefix = Self::new();
        let source = source.as_slice();
        // SAFETY: Prefix bounds the source by CAP and this fresh owner is disjoint.
        unsafe { buffer::raw::SliceExt::copy_to(source, prefix.bytes.as_mut_ptr()) };
        prefix.len = L::from_usize(source.len());
        prefix
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> PartialEq for Bytes<CAP, L> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> Eq for Bytes<CAP, L> {}

impl<const CAP: usize, L: crate::buffer::raw::Length> hash::Hash for Bytes<CAP, L> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state);
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> AsRef<[u8]> for Bytes<CAP, L> {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: construction and every append bound the initialized prefix by CAP.
        unsafe { self.bytes.get_unchecked(..self.len()) }
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> AsMut<[u8]> for Bytes<CAP, L> {
    fn as_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        // SAFETY: the exclusive owner retains its initialized prefix bounded by CAP.
        unsafe { self.bytes.get_unchecked_mut(..len) }
    }
}

impl<const CAP: usize, L: crate::buffer::raw::Length> write::Sink for Bytes<CAP, L> {
    type Error = Error;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.try_reserve(bytes.len())?
            .write(|output| output.copy_from_slice(bytes));
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_replace(self.len(), CAP, slices)
    }
}

// SAFETY: inline storage retains its prefix, and its Sink implementation only appends.
unsafe impl<const CAP: usize, L> buffer::raw::Source for Bytes<CAP, L> where
    Bytes<CAP, L>: AsRef<[u8]>
{
}
