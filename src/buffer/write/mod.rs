use std::{
    mem,
    num::NonZero,
    ptr,
    slice::{from_raw_parts_mut, from_ref},
};

pub use array::Array;
pub(in crate::buffer) use cursor::Cursor;
pub use reserved::Reserved;
pub use slice::Slice;
pub use staged::Staged;
use staged::publish_suffix;
pub(in crate::buffer) use staged::staging::Staging;

use crate::{
    alloc,
    alloc::VecExt as _,
    buffer::{self, write},
    mem::Rejected,
};

/// A destination whose individual writes append completely or not at all.
pub trait Sink {
    type Error;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.write_slice(from_ref(&byte))
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.write_slices([bytes])
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error>;
}

/// Checked transfers of a stable contiguous owner, fixed by the blanket implementation.
pub trait SourceExt: buffer::raw::Source + Sized {
    /// Copies exactly `source` into an empty owner, returning that owner on either outcome.
    /// Rejects its existing prefix before writing, or propagates the exact write failure.
    fn try_copy(mut self, source: &[u8]) -> Result<Self, (Self, CopyError<Self::Error>)>
    where
        Self: Sink,
    {
        if let Some(len) = NonZero::new(self.as_ref().len()) {
            return Err((self, CopyError::Prefix(len)));
        }
        if let Err(error) = self.write_slice(source) {
            return Err((self, CopyError::Write(error)));
        }
        Ok(self)
    }
}

impl<T: buffer::raw::Source> SourceExt for T {}

/// Failure to copy an exact source into an empty destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyError<E> {
    Prefix(NonZero<usize>),
    Write(E),
}

impl Sink for Vec<u8> {
    type Error = alloc::Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        use buffer::raw::SliceExt as _;

        let len = slices
            .iter()
            .try_fold(0usize, |len, slice| len.checked_add(slice.len()))
            .ok_or(alloc::Error::Overflow)?;
        alloc::VecExt::try_reserve(self, len)?;
        let start = self.len();
        // SAFETY: successful admission retains room for every original disjoint source.
        unsafe { slices.copy_to(self.as_mut_ptr().add(start)) };
        // SAFETY: every admitted source initialized its exact reserved suffix.
        unsafe { publish_suffix(self, len) };
        Ok(())
    }
}

mod array;
pub mod consume;
mod cursor;
pub mod edit;
mod reserved;
mod slice;
pub mod spans;
mod staged;

/// Rejected complete write, before either source or destination advances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Capacity(consume::Error),
    Overflow(Overflow),
}

const _: () = assert!(size_of::<Error>() == 3 * size_of::<usize>());

/// Exact operands of an unrepresentable combined write length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Overflow {
    pub length: usize,
    pub added: NonZero<usize>,
}

impl Overflow {
    fn add(length: usize, added: usize) -> Result<usize, Self> {
        length.checked_add(added).ok_or_else(|| Self {
            length,
            added: NonZero::<usize>::MIN.saturating_add(added - 1),
        })
    }
}

const _: () = assert!(size_of::<Overflow>() == 2 * size_of::<usize>());

/// Appends to one vector without exposing or removing its existing prefix.
#[derive(Debug)]
pub struct Append<'a, const N: usize = 0> {
    out: &'a mut Vec<u8>,
    start: usize,
}

impl<'a> Append<'a> {
    pub fn new(out: &'a mut Vec<u8>) -> Self {
        let start = out.len();
        Self { out, start }
    }

    pub fn append(&mut self) -> Append<'_> {
        Append::new(self.out)
    }

    /// Retains the owner and starts a new append at its current end.
    pub fn into_append(self) -> Self {
        Self::new(self.out)
    }

    pub fn try_fill(&mut self, len: usize, byte: u8) -> Result<(), alloc::Error> {
        crate::alloc::VecExt::try_reserve(self.out, len)?;
        // SAFETY: admission retains this exact exclusive suffix before writing it.
        unsafe {
            self.out
                .as_mut_ptr()
                .add(self.out.len())
                .write_bytes(byte, len)
        };
        // SAFETY: the complete admitted suffix is initialized above.
        unsafe { publish_suffix(self.out, len) };
        Ok(())
    }

    pub fn try_grow(&mut self, added: usize) -> Result<(), alloc::Error> {
        self.out.try_grow(added)
    }

    /// Appends the sources and padding while retaining capacity for a fixed tail.
    ///
    /// The returned owner exposes only initialized bytes until `commit` supplies
    /// the entire tail. A failure preserves the unchanged append owner.
    pub fn try_tail<const N: usize, const S: usize>(
        self,
        sources: [&[u8]; S],
        padding: usize,
        byte: u8,
    ) -> Result<Append<'a, N>, Rejected<Self, alloc::Error>> {
        use buffer::raw::SliceExt as _;

        let Some(length) = sources
            .iter()
            .try_fold(padding, |length, source| length.checked_add(source.len()))
            .and_then(|length| length.checked_add(N))
        else {
            return Err(Rejected::new(self, alloc::Error::Overflow));
        };
        if let Err(error) = self.out.try_grow(length) {
            return Err(Rejected::new(self, error));
        }
        let start = self.out.len();
        let copied = length - padding - N;
        // SAFETY: the checked total includes every source, padding, and fixed
        // tail on this exclusively retained vector. Sources cannot alias it.
        unsafe { sources.copy_to(self.out.as_mut_ptr().add(start)) };
        // SAFETY: the same admitted total leaves room for this exact padding.
        unsafe {
            self.out
                .as_mut_ptr()
                .add(start + copied)
                .write_bytes(byte, padding)
        };
        // SAFETY: the sources and padding above the unchanged length are initialized.
        unsafe { publish_suffix(self.out, copied + padding) };
        Ok(Append {
            out: self.out,
            start: self.start,
        })
    }
}

impl<'a, const N: usize> Append<'a, N> {
    pub fn len(&self) -> usize {
        self.out.len() - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.out.len() == self.start
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn commit(self, tail: &[u8; N]) -> Append<'a> {
        use buffer::raw::SliceExt as _;

        let end = self.out.len();
        // SAFETY: N = 0 needs no spare capacity. Otherwise try_tail retained N
        // bytes on this exact vector, and this state cannot change its extent.
        unsafe { tail.as_slice().copy_to(self.out.as_mut_ptr().add(end)) };
        // SAFETY: try_tail reserved exactly these N bytes for this commit.
        unsafe { publish_suffix(self.out, N) };
        Append {
            out: self.out,
            start: self.start,
        }
    }

    pub fn into_mut(self) -> &'a mut [u8] {
        // SAFETY: this exclusive append retains its original initialized prefix.
        unsafe { buffer::raw::range_mut(self.out.as_mut_ptr(), self.start, self.len()) }
    }

    pub fn rollback(self) {
        self.out.truncate(self.start);
    }
}

impl Sink for Append<'_> {
    type Error = alloc::Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.out.write_slices(slices)
    }
}

impl<const N: usize> AsRef<[u8]> for Append<'_, N> {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: this exclusive owner only appends; rollback consumes it and
        // nested appends cannot remove this owner's prefix or remain borrowed.
        unsafe { buffer::raw::range(self.out.as_ptr(), self.start, self.len()) }
    }
}

impl<const N: usize> AsMut<[u8]> for Append<'_, N> {
    fn as_mut(&mut self) -> &mut [u8] {
        // SAFETY: only this append's initialized suffix is exposed, never its prefix.
        unsafe { buffer::raw::range_mut(self.out.as_mut_ptr(), self.start, self.len()) }
    }
}

const _: () = {
    assert!(mem::size_of::<Append<'static>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Append<'static>>>() == mem::size_of::<Append<'static>>());
    assert!(!mem::needs_drop::<Append<'static>>());
    assert!(mem::size_of::<Append<'static, 16>>() == mem::size_of::<Append<'static>>());
};

/// Exclusive staged writer over one owner's uninitialized suffix.
#[must_use = "call commit to publish initialized bytes"]
#[repr(C)]
pub struct Spare<'a> {
    pub(in crate::buffer) pointer: ptr::NonNull<mem::MaybeUninit<u8>>,
    pub(in crate::buffer) target: &'a mut u32,
    pub(in crate::buffer) capacity: u32,
    pub(in crate::buffer) len: u32,
}

impl<'a> Spare<'a> {
    /// # Safety
    /// `spare` is this live owner's exclusive unpublished suffix for `'a`;
    /// `target` counts its initialized prefix and their total length fits `u32`.
    pub(in crate::buffer) unsafe fn from_parts(
        spare: &'a mut [mem::MaybeUninit<u8>],
        target: &'a mut u32,
    ) -> Self {
        Self {
            capacity: spare.len() as u32,
            pointer: ptr::NonNull::from(spare).cast(),
            target,
            len: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn remaining(&self) -> usize {
        buffer::raw::Space::remaining(self)
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        buffer::raw::Exclusive::as_mut_slice(self)
    }

    pub fn fill(&mut self, byte: u8) {
        buffer::raw::Exclusive::fill(self, byte)
    }

    /// Appends the largest source prefix that fits and returns its length.
    pub fn write(&mut self, src: &[u8]) -> usize {
        let (append, _) = Staging::prefix(self, src);
        let copied = append.len();
        buffer::raw::Copy::copy(append);
        copied
    }

    /// Publishes the largest fitting prefix and leaves the original source's tail.
    pub fn drain(mut self, source: &mut &[u8]) {
        let (append, tail) = Staging::prefix(&mut self, source);
        buffer::raw::Copy::copy(append);
        self.commit();
        *source = tail;
    }

    pub fn bounded(&mut self) -> spans::Writer<'_, 'a, false> {
        spans::Writer {
            cursor: Cursor::new(self),
        }
    }

    pub fn exact(&mut self) -> spans::Writer<'_, 'a, true> {
        spans::Writer {
            cursor: Cursor::new(self),
        }
    }

    pub fn try_exact<'writer>(
        &'writer mut self,
        len: usize,
    ) -> Result<spans::Writer<'writer, 'a, true>, consume::Error> {
        Cursor::try_new(self, len).map(|cursor| spans::Writer { cursor })
    }

    /// Admits the exact source before dependent work, then copies only on success.
    pub fn try_append<T, E: From<consume::Error>>(
        &mut self,
        bytes: &[u8],
        ready: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let append = Staging::try_new(self, [bytes])?;
        let value = ready()?;
        buffer::raw::Copy::copy(append);
        Ok(value)
    }

    /// Publishes this writer's complete initialized prefix exactly once.
    pub fn commit(self) {
        *self.target += self.len;
    }

    /// Publishes only when the entire original suffix has been initialized.
    pub fn try_complete(self) -> Result<(), spans::Gap> {
        spans::Gap::check(self.capacity, self.len)?;
        self.commit();
        Ok(())
    }
}

impl write::Sink for Spare<'_> {
    type Error = consume::Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        Staging::try_new(self, slices).map(buffer::raw::Copy::copy)
    }
}

const _: () = assert!(size_of::<Spare<'static>>() == size_of::<usize>() * 3);
const _: () = assert!(size_of::<Option<Spare<'static>>>() == size_of::<Spare<'static>>());

impl buffer::raw::Space for Spare<'_> {
    fn remaining(&self) -> usize {
        (self.capacity - self.len) as usize
    }
}

// SAFETY: Spare retains its original suffix and counts only initialized bytes.
unsafe impl buffer::raw::Prefix for Spare<'_> {
    fn prefix(&self) -> (*mut mem::MaybeUninit<u8>, u32) {
        (self.pointer.as_ptr(), self.len)
    }
}

// SAFETY: the staged length word is disjoint from the exclusive suffix;
// the original owner's length remains a separate publication target.
unsafe impl buffer::raw::Exclusive for Spare<'_> {
    fn as_mut_ptr(&mut self) -> *mut mem::MaybeUninit<u8> {
        self.pointer.as_ptr()
    }

    fn len_mut(&mut self) -> &mut u32 {
        &mut self.len
    }

    fn capacity(&self) -> u32 {
        self.capacity
    }
}

/// Owners whose exclusive unpublished suffix starts one staged writer.
pub trait SpareExt: Sized {
    /// Starts a staged writer over this owner's unpublished suffix.
    fn spare(&mut self) -> Spare<'_>;

    /// Stages the initialized prefix of `N` bytes at `offset` for editing.
    fn try_prefix<const N: usize>(
        &mut self,
        offset: usize,
    ) -> Result<edit::Edit<'_, N>, edit::Error>;
}

impl<T: buffer::raw::Exclusive> SpareExt for T {
    fn spare(&mut self) -> Spare<'_> {
        let (len, spare) = buffer::raw::Exclusive::spare_parts(self);
        // SAFETY: the owner retains both parts of its own exclusive suffix, and
        // its u32 capacity bounds their combined initialized length.
        unsafe { Spare::from_parts(spare, len) }
    }

    fn try_prefix<const N: usize>(
        &mut self,
        offset: usize,
    ) -> Result<edit::Edit<'_, N>, edit::Error> {
        let capacity = buffer::raw::Exclusive::capacity(self);
        let pointer = buffer::raw::Exclusive::as_mut_ptr(self);
        let len = buffer::raw::Exclusive::len_mut(self);
        let used = *len as usize;
        if offset > used || N > used - offset {
            return Err(edit::Error::new(offset, N, used));
        }
        // SAFETY: admission selected an initialized array; this owner retains
        // it exclusively and the unpublished suffix is disjoint from it.
        let (prefix, spare) = unsafe {
            (
                &mut *pointer.add(offset).cast::<[u8; N]>(),
                from_raw_parts_mut(pointer.add(used), capacity as usize - used),
            )
        };
        // SAFETY: the same owner provides this spare suffix and length word.
        let spare = unsafe { Spare::from_parts(spare, len) };
        Ok(edit::Edit::new(prefix, spare))
    }
}
