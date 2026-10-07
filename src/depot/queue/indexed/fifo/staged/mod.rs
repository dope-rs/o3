//! Indexed FIFO with a key retained across queued and parked payload phases.

use std::{marker, mem, num};

pub use entry::Entry;
pub use member::Member;
pub(in crate::depot) use phase::Phase;
pub use state::State;
pub use table::Table;

use crate::depot::{
    queue::{indexed::fifo::staged, links},
    raw::Live,
};

mod entry;
mod member;
mod state;

mod phase;
mod table;

/// Exclusive access to the complete staged table retained by this source.
/// No access may overlap any derived entry or slot, including callback reentry.
#[derive(Debug)]
pub struct Exclusive;

/// One represented slot and the facts retained by its exclusive phase.
#[must_use]
pub struct Slot<'a, K, Q, P, S, I: crate::depot::raw::Compact = u32> {
    pub(in crate::depot) owner: &'a staged::Table<K, Q, P, I>,
    pub(in crate::depot) index: links::Index,
    pub(in crate::depot) phase: S,
    pub(in crate::depot) loan: marker::PhantomData<&'a mut staged::Table<K, Q, P, I>>,
}

/// The queued payload phase retaining its exact predecessor.
#[repr(transparent)]
pub struct Queued(pub(in crate::depot) links::Index);

/// The parked payload phase retaining its exact predecessor.
#[repr(transparent)]
pub struct Parked(pub(in crate::depot) links::Index);

impl<'a, K, Q, P, S: crate::depot::raw::StagedPhase<K, Q, P>, I: crate::depot::raw::Compact>
    Slot<'a, K, Q, P, S, I>
{
    pub fn first(
        source: crate::depot::raw::Proof<&'a staged::Table<K, Q, P, I>, staged::Exclusive>,
    ) -> Option<Self> {
        let owner = source.into_inner();
        let state = owner.nodes.state();
        let index = if S::QUEUED {
            state.head.get()?
        } else {
            state.parked.get()?
        };
        Some(Self {
            owner,
            index,
            phase: S::new(index),
            loan: marker::PhantomData,
        })
    }

    /// Projects an occupied slot while preserving its original source capability.
    pub fn from_source<C>(source: Admission<'a, K, Q, P, I, C, Self>) -> (Self, C) {
        let ((owner, context), position) = source.into_inner();
        let index = links::Index::from(position);
        // SAFETY: Live retains this exact occupied phase and original owner.
        let node = unsafe { crate::depot::raw::StagedNode::at(owner, index) };
        (
            Self {
                owner,
                index,
                phase: S::new(node.previous.get()),
                loan: marker::PhantomData,
            },
            context,
        )
    }

    pub fn key(&self) -> &K {
        &S::payload(self).key
    }
    pub fn remove(self) -> Removal<'a, K, Q, P, I, S::Value> {
        crate::depot::raw::Retired::<_, _, _, S, _>::new(self).into_parts()
    }

    pub fn take(self) -> (K, S::Value) {
        self.remove().1
    }

    pub fn clear(self) {
        crate::depot::raw::Retired::<_, _, _, S, _>::new(self).destroy();
    }
}

impl<K, Q, P, S: crate::depot::raw::StagedPhase<K, Q, P>, I: crate::depot::raw::Compact>
    AsRef<S::Value> for Slot<'_, K, Q, P, S, I>
{
    fn as_ref(&self) -> &S::Value {
        S::value(self)
    }
}

impl<K, Q, P, S: crate::depot::raw::StagedPhase<K, Q, P>, I: crate::depot::raw::Compact>
    AsMut<S::Value> for Slot<'_, K, Q, P, S, I>
{
    fn as_mut(&mut self) -> &mut S::Value {
        S::value_mut(self)
    }
}

impl<K, Q, P, S, I: crate::depot::raw::Compact> Slot<'_, K, Q, P, S, I> {
    pub fn index(&self) -> I {
        // SAFETY: this represented slot retains the owner's admitted index domain.
        unsafe { I::from_raw(self.index.raw()) }
    }
    pub fn position(&self) -> num::NonZero<u32> {
        self.index.position()
    }
}

impl<'a, K, Q, P, I: crate::depot::raw::Compact> Slot<'a, K, Q, P, (), I> {
    pub fn push_back(self, key: K, value: Q) -> Slot<'a, K, Q, P, staged::Queued, I> {
        crate::depot::raw::StagedMove::insert(self, key, value).commit()
    }
    pub fn insert(self, key: K, value: Q) {
        crate::depot::raw::StagedMove::insert(self, key, value).store();
    }
    pub(in crate::depot) fn release(self, occupied: num::NonZero<u32>) -> Self {
        let node = crate::depot::raw::StagedNode::of(&self);
        node.phase.set(staged::Phase::Vacant);
        node.next.set(None);
        self.owner.nodes.state().occupied.set(occupied.get() - 1);
        self
    }
}

impl<'a, K, Q, P, I: crate::depot::raw::Compact> Slot<'a, K, Q, P, Queued, I> {
    pub fn park(self, value: P) -> (Slot<'a, K, Q, P, staged::Parked, I>, Q) {
        crate::depot::raw::StagedMove::replace(self, value)
    }
    pub fn set_parked(self, value: P) -> Q {
        self.park(value).1
    }
}

impl<'a, K, Q, P, I: crate::depot::raw::Compact> Slot<'a, K, Q, P, Parked, I> {
    pub fn resume(self, value: Q) -> (Slot<'a, K, Q, P, staged::Queued, I>, P) {
        crate::depot::raw::StagedMove::replace(self, value)
    }
}

const _: () = {
    assert!(mem::size_of::<Slot<'_, (), (), (), Queued>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Slot<'_, (), (), (), Queued>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Slot<'_, (), (), (), Parked>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Slot<'_, (), (), (), Parked>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Slot<'_, (), (), (), ()>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Slot<'_, (), (), (), ()>>>() == 2 * mem::size_of::<usize>());
};

impl crate::depot::Predicate for Exclusive {}

/// Staged admission and removal preserve the original table and owned payload.
type Admission<'a, K, Q, P, I, C, F> = Live<(&'a mut Table<K, Q, P, I>, C), num::NonZero<u32>, F>;
type Removal<'a, K, Q, P, I, V> = (Slot<'a, K, Q, P, (), I>, (K, V));
