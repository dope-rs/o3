use std::{iter, mem, num::NonZero, ops};

pub use prefix::Prefix;
pub(crate) use retain::Retain;
use storage::range;

use crate::{
    buffer,
    buffer::{Complete as _, hold, pool, pool::owners, resident, storage, write::consume},
};

pub mod bounded;
mod prefix;
mod retain;
pub mod scope;
pub mod window;
/// Read-only bytes with an explicit fallible transition beyond their callback.
pub trait TryRetain: Retain + AsRef<[u8]> + Sized {
    fn try_retain(self) -> Result<storage::Shared, storage::AllocError>;
}
/// An explicit choice between borrowed bytes and an independently retained result.
pub enum Source<'a, R: TryRetain> {
    Borrowed(&'a [u8]),
    Owned(R),
}
impl<R: TryRetain> AsRef<[u8]> for Source<'_, R> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Owned(bytes) => bytes.as_ref(),
        }
    }
}
impl TryRetain for &[u8] {
    fn try_retain(self) -> Result<storage::Shared, storage::AllocError> {
        storage::Shared::try_copy(self)
    }
}
impl<R: TryRetain> TryRetain for Source<'_, R> {
    fn try_retain(self) -> Result<storage::Shared, storage::AllocError> {
        match self {
            Self::Borrowed(bytes) => bytes.try_retain(),
            Self::Owned(bytes) => bytes.try_retain(),
        }
    }
}

/// A pooled read transaction; dropping it leaves the original owner unchanged.
pub struct Cursor<'a, 'pool> {
    pub(in crate::buffer) target:
        &'a mut storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>>,
    pub(in crate::buffer) consumed: usize,
}

impl<'a, 'pool> Cursor<'a, 'pool> {
    pub fn new(
        target: &'a mut storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>>,
    ) -> Self {
        Self {
            target,
            consumed: 0,
        }
    }

    pub fn commit(self) {
        let len = self.target.len();
        let target = &mut *self.target;
        // SAFETY: the transaction tracks a consumed prefix of this unchanged owner.
        unsafe { target.region.restrict(self.consumed..len) };
        if target.is_empty() {
            *self.target = storage::Shared::empty(owners::Edge::NONE);
        }
    }
}

impl<'pool> buffer::Source for Cursor<'_, 'pool> {
    type View<'input>
        = buffer::View<'input, storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>>>
    where
        Self: 'input;

    fn remaining(&self) -> usize {
        self.target.len() - self.consumed
    }
}

impl AsRef<[u8]> for Cursor<'_, '_> {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: `consumed` advances only through checked cursor operations.
        unsafe { self.target.as_slice().get_unchecked(self.consumed..) }
    }
}

impl consume::Consume for Cursor<'_, '_> {
    fn remaining(&self) -> usize {
        self.target.len() - self.consumed
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        consume::Error::check(amount, self.target.len() - self.consumed)?;
        self.consumed += amount;
        Ok(())
    }

    fn consume_max(&mut self, max: usize) -> usize {
        let amount = max.min(self.target.len() - self.consumed);
        self.consumed += amount;
        amount
    }
}

// SAFETY: the target remains exclusively borrowed until commit or cancellation.
unsafe impl buffer::raw::Consume for Cursor<'_, '_> {
    fn consume<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        let (cursor, end) = prefix.into_parts();
        cursor.consumed += end.count().get() as usize;
    }
}

impl buffer::Cursor for Cursor<'_, '_> {
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error> {
        self.view().try_get(range)
    }
}

impl<'pool> buffer::Complete for Cursor<'_, 'pool> {
    fn view(&self) -> Self::View<'_> {
        buffer::View {
            source: self.target,
            bytes: self.as_ref(),
        }
    }
}

// SAFETY: The selected immutable pool view delegates retention to its original owner.
unsafe impl<'pool> hold::raw::Retain<'pool> for Cursor<'_, 'pool> {
    fn charge<'a>(plan: &Self::View<'a>) -> hold::Charge
    where
        Self: 'a,
    {
        <storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>> as hold::raw::Retain<
            'pool,
        >>::charge(plan)
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'pool>,
    ) -> Result<resident::Shared<'pool>, hold::Error>
    where
        Self: 'a,
    {
        <storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>> as hold::raw::Retain<
            'pool,
        >>::try_retain(plan, budget)
    }
}

const _: () = assert!(size_of::<Cursor<'static, 'static>>() == 2 * size_of::<usize>());

/// Disjoint mutable chunks retaining their admitted nonzero width.
#[derive(Debug)]
pub struct ChunksMut<'a> {
    remaining: &'a mut [u8],
    size: NonZero<usize>,
}

impl<'a> ChunksMut<'a> {
    pub fn new(bytes: &'a mut [u8], size: NonZero<usize>) -> Self {
        Self {
            remaining: bytes,
            size,
        }
    }
}

impl<'a> Iterator for ChunksMut<'a> {
    type Item = &'a mut [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let end = self.size.get().min(self.remaining.len());
        let (chunk, tail) = mem::take(&mut self.remaining).split_at_mut(end);
        self.remaining = tail;
        Some(chunk)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for ChunksMut<'_> {
    fn len(&self) -> usize {
        self.remaining.len().div_ceil(self.size.get())
    }
}

impl iter::FusedIterator for ChunksMut<'_> {}

const _: () = assert!(size_of::<ChunksMut<'static>>() == 3 * size_of::<usize>());
