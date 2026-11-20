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

impl<'a, K, Q, P, S: crate::depot::raw::StagedPhase<Q, P>, I: crate::depot::raw::Compact>
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

const _: () = {
    assert!(mem::size_of::<Slot<'_, (), (), (), Queued>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Slot<'_, (), (), (), Queued>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Slot<'_, (), (), (), Parked>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Slot<'_, (), (), (), Parked>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Slot<'_, (), (), (), ()>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Slot<'_, (), (), (), ()>>>() == 2 * mem::size_of::<usize>());
};

type Admission<'a, K, Q, P, I, C, F> = Live<(&'a mut Table<K, Q, P, I>, C), num::NonZero<u32>, F>;
