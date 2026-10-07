use std::{marker, mem};

use crate::depot::{
    arena,
    queue::{indexed::fifo::staged, links},
};

/// Owns a replacement payload while its original slot changes topology.
#[must_use]
pub(in crate::depot) struct StagedMove<'a, K, Q, P, S, I: crate::depot::raw::Compact> {
    slot: staged::Slot<'a, K, Q, P, (), I>,
    phase: marker::PhantomData<S>,
}

impl<'a, K, Q, P, I: crate::depot::raw::Compact> StagedMove<'a, K, Q, P, staged::Queued, I> {
    /// Initializes the exclusively reserved payload.
    /// The vacant slot proves `occupied < capacity <= u32::MAX`.
    pub(in crate::depot) fn insert(
        slot: staged::Slot<'a, K, Q, P, (), I>,
        key: K,
        value: Q,
    ) -> Self {
        let count = &slot.owner.nodes.state().occupied;
        // SAFETY: this exclusive vacant position is outside the initialized
        // count and owns its uninitialized payload until the consuming commit.
        unsafe {
            crate::depot::raw::StagedNode::of(&slot)
                .value
                .get()
                .write(mem::MaybeUninit::new(arena::raw::StagedValue::queued(
                    key, value,
                )));
        }
        count.set(count.get() + 1);
        Self {
            slot,
            phase: marker::PhantomData,
        }
    }
}

impl<'a, K, Q, P, S: crate::depot::raw::StagedPhase<K, Q, P>, I: crate::depot::raw::Compact>
    StagedMove<'a, K, Q, P, S, I>
{
    pub(in crate::depot) fn replace<Old: crate::depot::raw::StagedPhase<K, Q, P>>(
        mut slot: staged::Slot<'a, K, Q, P, Old, I>,
        value: S::Value,
    ) -> (staged::Slot<'a, K, Q, P, S, I>, Old::Value) {
        // SAFETY: the exclusive source phase owns the active member. Taking it
        // and writing the destination member leaves the retained key unchanged.
        let payload = unsafe { Old::payload_mut(&mut slot) };
        let prior = unsafe { Old::take(payload) };
        unsafe { S::write(payload, value) };
        // SAFETY: no callback or exposed vacancy intervenes before the new
        // payload is linked; this owner retains the unchanged occupied count.
        let slot = unsafe { Old::unlink(slot) };
        (
            Self {
                slot,
                phase: marker::PhantomData,
            }
            .commit(),
            prior,
        )
    }

    pub(in crate::depot) fn commit(self) -> staged::Slot<'a, K, Q, P, S, I> {
        let owner = self.slot.owner;
        let index = self.slot.index;
        let loan = self.slot.loan;
        let previous = self.store();
        staged::Slot {
            owner,
            index,
            phase: S::new(previous),
            loan,
        }
    }

    pub(in crate::depot) fn store(self) -> links::Index {
        let slot = self.slot;
        let state = slot.owner.nodes.state();
        let previous;
        let next;
        if S::QUEUED {
            previous = match state.tail.get() {
                Some(previous) => {
                    // SAFETY: the retained destination owns this reciprocal tail.
                    unsafe { crate::depot::raw::StagedNode::at(slot.owner, previous) }
                        .next
                        .set(Some(slot.index));
                    previous
                }
                None => {
                    state.head.set(Some(slot.index));
                    slot.index
                }
            };
            next = None;
            state.tail.set(Some(slot.index));
        } else {
            previous = slot.index;
            next = state.parked.get();
            if let Some(next) = next {
                // SAFETY: the retained parked chain owns this initialized head.
                unsafe { crate::depot::raw::StagedNode::at(slot.owner, next) }
                    .previous
                    .set(slot.index);
            }
            state.parked.set(Some(slot.index));
        }
        let node = crate::depot::raw::StagedNode::of(&slot);
        node.previous.set(previous);
        node.next.set(next);
        node.phase.set(if S::QUEUED {
            staged::Phase::Queued
        } else {
            staged::Phase::Parked
        });
        previous
    }
}
