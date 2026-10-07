use std::{cell, mem};

use crate::depot::{
    arena,
    queue::{indexed::fifo::staged, links},
};

#[repr(C)]
pub(in crate::depot) struct StagedNode<K, Q, P> {
    pub(in crate::depot) value:
        cell::UnsafeCell<mem::MaybeUninit<arena::raw::StagedValue<K, Q, P>>>,
    pub(in crate::depot) previous: cell::Cell<links::Index>,
    pub(in crate::depot) next: cell::Cell<Option<links::Index>>,
    pub(in crate::depot) phase: cell::Cell<staged::Phase>,
}

impl<K, Q, P> StagedNode<K, Q, P> {
    pub(in crate::depot) fn of<'a, S, I: crate::depot::raw::Compact>(
        slot: &staged::Slot<'a, K, Q, P, S, I>,
    ) -> &'a Self {
        // SAFETY: Slot retains this represented position and its original owner.
        unsafe { Self::at(slot.owner, slot.index) }
    }

    /// # Safety
    /// The position belongs to this table's immutable extent.
    pub(in crate::depot) unsafe fn at<I: crate::depot::raw::Compact>(
        owner: &staged::Table<K, Q, P, I>,
        index: links::Index,
    ) -> &Self {
        unsafe { owner.nodes.get_unchecked(index.get()) }
    }

    pub(in crate::depot) const fn new(index: links::Index) -> Self {
        Self {
            value: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
            previous: cell::Cell::new(index),
            next: cell::Cell::new(None),
            phase: cell::Cell::new(staged::Phase::Vacant),
        }
    }

    pub(in crate::depot) fn state(&self) -> staged::State<'_, K> {
        match self.phase.get() {
            staged::Phase::Queued => staged::State::Queued {
                key: unsafe { (&*self.value.get()).assume_init_ref() }.key(),
            },
            staged::Phase::Parked => staged::State::Parked {
                key: unsafe { (&*self.value.get()).assume_init_ref() }.key(),
            },
            staged::Phase::Vacant => staged::State::Vacant,
        }
    }

    pub(in crate::depot) fn parked(&self) -> Option<(&K, &P)> {
        match self.phase.get() {
            staged::Phase::Parked => {
                let value = unsafe { (&*self.value.get()).assume_init_ref() };
                Some((value.key(), unsafe { value.parked_ref() }))
            }
            staged::Phase::Queued | staged::Phase::Vacant => None,
        }
    }
}

const _: () =
    assert!(mem::size_of::<StagedNode<u64, [u64; 2], [u64; 5]>>() == 8 * mem::size_of::<u64>());
