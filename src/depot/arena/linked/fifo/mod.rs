//! Counted FIFO state over external linked slots.

use std::mem;

use crate::{depot, depot::arena::linked::slots};

pub mod ends;

/// Compact counted FIFO state stored outside its linked node arena.
#[repr(transparent)]
pub struct Fifo<Tag = ()> {
    pub(in crate::depot::arena::linked) state: slots::State<Tag>,
}

impl<Tag> Default for Fifo<Tag> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Tag> Fifo<Tag> {
    pub const fn new() -> Self {
        Self {
            state: slots::State::new(),
        }
    }

    pub const fn len(&self) -> usize {
        self.state.count().get()
    }

    pub const fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    pub fn back(&self) -> Option<Back<Tag>> {
        let index = slots::Index::new(self.state.back_index()?);
        Some(Back { index })
    }
}

const _: () = assert!(mem::size_of::<Fifo>() == 3 * mem::size_of::<u32>());

/// One observed live FIFO tail in a linked-node domain.
#[must_use]
#[repr(transparent)]
pub struct Back<Tag> {
    index: slots::Index<Tag>,
}

impl<Tag> Back<Tag> {
    pub(in crate::depot::arena::linked) const fn into_index(self) -> slots::Index<Tag> {
        self.index
    }
}

const _: () = assert!(mem::size_of::<Back<()>>() == mem::size_of::<u32>());

impl<Tag> Fifo<Tag> {
    /// # Safety
    /// This state owns one acyclic live partition in this exact store.
    pub(in crate::depot) unsafe fn queue<'a, T>(
        &'a mut self,
        store: &'a slots::Store<T>,
    ) -> slots::Bound<'a, T, Tag> {
        unsafe { self.state.bind(store) }
    }
}

/// The retained front is current, or the retained FIFO may be empty.
/// The callback detaches that exact stable front, returns its exact stored successor,
/// and neither reenters nor invalidates the retained endpoints during the operation.
#[derive(Debug)]
pub enum Detach {}

/// The retained index is a valid detached slot disjoint from the stable destination chain.
/// The callback stores the exact former front as its successor before publication,
/// without reentry or invalidating either the slot or destination endpoints.
#[derive(Debug)]
pub enum Prepend {}

/// The retained index is a valid detached tail slot disjoint from the stable destination chain.
/// The callback links the exact former tail to it before endpoint publication,
/// without reentry or invalidating either the slot or destination endpoints.
#[derive(Debug)]
pub enum Append {}

/// Both retained chains are stable, valid, disjoint partitions in one exact slot store.
/// The callback links the destination's exact former tail to the source's exact front,
/// without reentry or invalidating either chain before their endpoints are published.
#[derive(Debug)]
pub enum Join {}

impl depot::raw::Sealed for Detach {}
impl depot::Predicate for Detach {}
impl depot::raw::Sealed for Prepend {}
impl depot::Predicate for Prepend {}
impl depot::raw::Sealed for Append {}
impl depot::Predicate for Append {}
impl depot::raw::Sealed for Join {}
impl depot::Predicate for Join {}
