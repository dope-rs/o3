use std::{marker, mem};

use crate::{
    collections::{self, batch},
    mem::quota,
};

#[repr(transparent)]
pub struct Set<I = usize> {
    pub(super) raw: batch::RawSet,
    index: marker::PhantomData<fn(I) -> I>,
}

#[must_use]
pub struct Front<'a, I = usize> {
    set: &'a mut batch::RawSet,
    side: batch::Side,
    raw: usize,
    index: marker::PhantomData<fn(I) -> I>,
}

/// A stable dense index accepted and yielded by a [`Set`].
pub trait DenseIndex: Copy {
    fn into_usize(self) -> usize;
    fn from_usize(raw: usize) -> Self;
}

impl DenseIndex for usize {
    fn into_usize(self) -> usize {
        self
    }

    fn from_usize(raw: usize) -> Self {
        raw
    }
}

impl DenseIndex for u8 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for u16 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for u32 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for u64 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for i8 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for i16 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for i32 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for i64 {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

impl DenseIndex for isize {
    fn into_usize(self) -> usize {
        self as usize
    }

    fn from_usize(raw: usize) -> Self {
        raw as Self
    }
}

/// A consuming iterator over one stable [`Set`] batch.
#[repr(transparent)]
pub struct Drain<'a, I = usize> {
    raw: batch::RawDrain<'a>,
    index: marker::PhantomData<fn(I) -> I>,
}

#[must_use = "the result determines whether the ready index was consumed"]
pub enum Next<I> {
    Item(I),
    Empty,
    Exhausted(I),
}

impl<I> Set<I> {
    pub fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(set) => set,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            raw: batch::RawSet::try_with_capacity(capacity)?,
            index: marker::PhantomData,
        })
    }

    pub fn capacity(&self) -> usize {
        self.raw.capacity()
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

impl<I: DenseIndex> Set<I> {
    /// Inserts unless the index is out of capacity or already present.
    pub fn insert(&self, index: I) -> bool {
        self.raw.insert(index.into_usize())
    }

    pub fn contains(&self, index: I) -> bool {
        self.raw.contains(index.into_usize())
    }

    pub fn remove(&self, index: I) -> bool {
        self.raw.remove(index.into_usize())
    }

    pub fn pop(&self) -> Option<I> {
        self.raw.pop().map(I::from_usize)
    }

    pub fn front(&mut self) -> Option<Front<'_, I>> {
        let side = self.raw.active.get();
        let raw = self.raw.peek_side(side)?;
        Some(Front {
            set: &mut self.raw,
            side,
            raw,
            index: marker::PhantomData,
        })
    }

    /// Pops at or after `start`, wrapping once.
    pub fn pop_from(&self, start: I) -> Option<I> {
        self.raw.pop_from(start.into_usize()).map(I::from_usize)
    }

    /// Starts or resumes an isolated stable batch.
    pub fn drain_batch(&self) -> Option<Drain<'_, I>> {
        Some(Drain {
            raw: self.raw.drain_batch()?,
            index: marker::PhantomData,
        })
    }
}

impl<I: DenseIndex> Front<'_, I> {
    pub fn take(self) -> I {
        self.set.take(self.side, self.raw);
        I::from_usize(self.raw)
    }
}

impl<I: DenseIndex> Drain<'_, I> {
    pub fn peek(&self) -> Option<I> {
        self.raw.peek().map(I::from_usize)
    }

    pub fn next_with_quota<Tag>(&mut self, quota: &quota::Ledger<Tag>) -> Next<I> {
        let Some(raw) = self.raw.peek() else {
            return Next::Empty;
        };
        if !quota.take() {
            return Next::Exhausted(I::from_usize(raw));
        }
        self.raw.set.take(self.raw.side, raw);
        Next::Item(I::from_usize(raw))
    }

    /// Preserves the unconsumed entries for the next drain.
    pub fn pause(self) {
        self.raw.pause();
    }
}

impl<I: DenseIndex> Iterator for Drain<'_, I> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        self.raw.next().map(I::from_usize)
    }
}

const _: () = {
    assert!(mem::size_of::<Set<usize>>() == mem::size_of::<batch::RawSet>());
    assert!(mem::align_of::<Set<usize>>() == mem::align_of::<batch::RawSet>());
    assert!(mem::size_of::<Drain<'static, usize>>() == mem::size_of::<batch::RawDrain<'static>>());
    assert!(
        mem::align_of::<Drain<'static, usize>>() == mem::align_of::<batch::RawDrain<'static>>()
    );
};
