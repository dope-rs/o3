use std::{hash, mem};

pub use append::Append;

use crate::{
    buffer,
    buffer::{bytes, storage::inline, write, write::consume::Error},
};

pub const CAPACITY: usize = 24;

mod append;

struct Range<const CAP: usize> {
    start: usize,
    end: usize,
}

impl<const CAP: usize> Range<CAP> {
    fn try_new<L: crate::buffer::raw::Length>(
        target: &Bytes<CAP, L>,
        amount: usize,
    ) -> Result<Self, Error> {
        let start = target.len();
        Error::check(amount, CAP - start)?;
        Ok(Self {
            start,
            end: start + amount,
        })
    }

    fn try_slices<const N: usize>(
        start: usize,
        limit: usize,
        slices: &[&[u8]; N],
    ) -> Result<Self, Error> {
        let end = Error::checked_end(start, limit.min(CAP), slices)?;
        Ok(Self { start, end })
    }

    fn fit<L: crate::buffer::raw::Length>(target: &Bytes<CAP, L>, amount: usize) -> Self {
        let start = target.len();
        Self {
            start,
            end: start + amount.min(CAP - start),
        }
    }

    fn bind<L: crate::buffer::raw::Length>(
        self,
        target: &mut inline::Bytes<CAP, L>,
    ) -> inline::Append<'_, L> {
        // SAFETY: every Range constructor proves the selected extent lies within CAP.
        let output = unsafe { target.bytes.get_unchecked_mut(self.start..self.end) };
        inline::Append::new(output, &mut target.len)
    }
}

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
        let end: Range<CAP> = Range::<CAP>::try_new(self, amount)?;
        Ok(end.bind(self))
    }

    /// Reserves the largest prefix up to `amount` that currently fits.
    pub fn fit(&mut self, amount: usize) -> inline::Append<'_, L> {
        let end = Range::<CAP>::fit(self, amount);
        end.bind(self)
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
        let range = Range::<CAP>::try_slices(start.min(self.len()), limit, &slices)?;
        buffer::raw::Copy::copy(Writing {
            target: self,
            sources: slices,
            range,
        });
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
        let range = Range::<CAP>::try_new(self, bytes.len())?;
        buffer::raw::Copy::copy(Writing {
            target: self,
            sources: bytes,
            range,
        });
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_replace(self.len(), CAP, slices)
    }
}

/// Exact source bytes and their admitted range in one exclusively retained owner.
struct Writing<'target, const CAP: usize, L, S> {
    target: &'target mut Bytes<CAP, L>,
    sources: S,
    range: Range<CAP>,
}

// SAFETY: admission retains the same disjoint sources, exact destination range,
// and exclusive owner; only their complete copy publishes the admitted end.
unsafe impl<const CAP: usize, L: buffer::raw::Length, S: buffer::raw::SliceExt> buffer::raw::Copy
    for Writing<'_, CAP, L, S>
{
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (
            &self.sources,
            self.target.bytes.as_mut_ptr(),
            self.range.start,
        )
    }

    unsafe fn publish(self) {
        self.target.len = L::from_usize(self.range.end);
    }
}

// SAFETY: inline storage retains its prefix, and its Sink implementation only appends.
unsafe impl<const CAP: usize, L> buffer::raw::Source for Bytes<CAP, L> where
    Bytes<CAP, L>: AsRef<[u8]>
{
}
