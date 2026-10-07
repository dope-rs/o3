//! Compact external forward-chain states over one exact fixed node store.

use std::{iter, mem};

use crate::depot;

pub mod chain;
pub mod fifo;
pub mod head;
pub mod slots;

/// Immutable forward traversal retaining the linked store lifetime.
///
/// The traversal also retains the borrow of its exact topology owner.
pub struct Iter<'a, T, Tag = ()> {
    store: &'a slots::Store<T>,
    current: slots::Index<Tag>,
}

impl<'a, T, Tag> Iter<'a, T, Tag> {
    /// # Safety
    /// Reachable nodes form one acyclic live partition of `store`; values remain
    /// shared and initialized and links unchanged for `'a`, even if the store outlives it.
    pub(crate) unsafe fn new(store: &'a slots::Store<T>, current: slots::Index<Tag>) -> Self {
        Self { store, current }
    }

    /// # Safety
    /// The traversal's current index names a live node in its admitted partition.
    pub(crate) unsafe fn next_live(&mut self) -> &'a T {
        let item = unsafe { self.store.value(self.current) };
        self.current = unsafe { self.store.next(self.current) };
        item
    }
}

impl<'a, T: 'a, Tag> Iterator for Iter<'a, T, Tag> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current == slots::Index::NONE {
            return None;
        }
        // SAFETY: construction retains the exact immutable live partition,
        // and the sentinel check proves that its current node is present.
        Some(unsafe { self.next_live() })
    }
}

impl<T, Tag> iter::FusedIterator for Iter<'_, T, Tag> {}

impl<T, Tag> Clone for Iter<'_, T, Tag> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, Tag> Copy for Iter<'_, T, Tag> {}

const _: () = assert!(mem::size_of::<Iter<'static, u64, ()>>() == 2 * mem::size_of::<usize>());

/// A closed owner retaining the exact store, accounting policy, and return rights.
pub trait Source<T>: Sealed<T> {
    const TRACK: bool = true;
    fn store(&self) -> &slots::Store<T>;
}

impl<T, S: Source<T>> Source<T> for &S {
    const TRACK: bool = S::TRACK;

    fn store(&self) -> &slots::Store<T> {
        S::store(self)
    }
}

pub(in crate::depot) use sealed::Sealed;
mod sealed;

/// Admits one traversal's peak slot claims and returns unused credits on completion.
pub trait Batch<'a, T: 'a, Tag: 'a>: depot::raw::LinkedBatch<'a, T, Tag> {
    fn try_apply(self) -> Result<Self::Output, Self::Error> {
        let store = &self.nodes().inner;
        let count = self.count();
        if let Err(error) = store.try_reserve(count) {
            return Err(self.reject(error));
        }
        let mut reserve = slots::Reserve::new(store, count);
        Ok(self.run(&mut reserve))
    }
}

impl<'a, T: 'a, Tag: 'a, B: depot::raw::LinkedBatch<'a, T, Tag>> Batch<'a, T, Tag> for B {}

/// Both retained capabilities belong to the same exact linked Store and slot namespace.
/// Their native phases, lifetimes, and sole return rights remain unchanged until consumption.
#[derive(Debug)]
pub enum Same {}

/// The retained exclusive Reserve borrow owns at least one unconsumed reserved slot credit.
/// That exact credit remains available until the proof is consumed.
#[derive(Debug)]
pub enum Available {}

impl depot::raw::Sealed for Same {}
impl depot::Predicate for Same {}
impl depot::raw::Sealed for Available {}
impl depot::Predicate for Available {}
