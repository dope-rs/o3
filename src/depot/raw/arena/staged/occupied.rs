use std::num;

use staged::Slot;

use crate::depot::{
    arena,
    queue::{indexed::fifo::staged, links},
};

/// The active union member and topology retained by one occupied Slot phase.
/// # Safety
/// QUEUED/previous identify the exact member/chain; projections retain the borrow; take/write/clear change only that member; unlink changes only topology; none may reenter the owner.
pub unsafe trait StagedPhase<K, Q, P>: Sized {
    type Value;
    const QUEUED: bool;
    fn new(previous: links::Index) -> Self;
    fn previous(&self) -> links::Index;
    fn value<'s, I: crate::depot::raw::Compact>(
        slot: &'s Slot<'_, K, Q, P, Self, I>,
    ) -> &'s Self::Value;
    fn value_mut<'s, I: crate::depot::raw::Compact>(
        slot: &'s mut Slot<'_, K, Q, P, Self, I>,
    ) -> &'s mut Self::Value;
    /// # Safety
    /// This phase owns the active value, which must be consumed exactly once.
    unsafe fn take(payload: &mut arena::raw::StagedValue<K, Q, P>) -> Self::Value;
    /// # Safety
    /// The payload retains its key and exclusively owns an inactive value union.
    unsafe fn write(payload: &mut arena::raw::StagedValue<K, Q, P>, value: Self::Value);
    /// # Safety
    /// This phase owns the initialized key and value, both dropped exactly once.
    unsafe fn clear(payload: &mut arena::raw::StagedValue<K, Q, P>);
    /// # Safety
    /// Immediately finish this exact slot's payload, phase, and count transition;
    /// its provisional vacancy must not be exposed or dropped before completion.
    unsafe fn unlink<I: crate::depot::raw::Compact>(
        slot: Slot<'_, K, Q, P, Self, I>,
    ) -> Slot<'_, K, Q, P, (), I> {
        let state = slot.owner.nodes.state();
        let node = crate::depot::raw::StagedNode::of(&slot);
        let next = node.next.get();
        let previous = slot.phase.previous();
        let first = previous == slot.index;
        if first {
            let head = if Self::QUEUED {
                &state.head
            } else {
                &state.parked
            };
            head.set(next);
        } else {
            // SAFETY: this occupied phase retains its reciprocal predecessor.
            unsafe { crate::depot::raw::StagedNode::at(slot.owner, previous) }
                .next
                .set(next);
        }
        if let Some(next) = next {
            // SAFETY: this occupied phase retains its reciprocal successor.
            unsafe { crate::depot::raw::StagedNode::at(slot.owner, next) }
                .previous
                .set(if first { next } else { previous });
        }
        if Self::QUEUED && next.is_none() {
            state.tail.set(if first { None } else { Some(previous) });
        }
        Slot {
            owner: slot.owner,
            index: slot.index,
            phase: (),
            loan: slot.loan,
        }
    }

    fn occupied<I: crate::depot::raw::Compact>(
        slot: &Slot<'_, K, Q, P, Self, I>,
    ) -> num::NonZero<u32> {
        // SAFETY: this phase retains one initialized payload in its exact owner.
        unsafe { num::NonZero::new_unchecked(slot.owner.nodes.state().occupied.get()) }
    }

    fn payload<'s, I: crate::depot::raw::Compact>(
        slot: &'s Slot<'_, K, Q, P, Self, I>,
    ) -> &'s arena::raw::StagedValue<K, Q, P> {
        // SAFETY: an occupied Slot owns the initialized member selected by S.
        unsafe { (&*crate::depot::raw::StagedNode::of(slot).value.get()).assume_init_ref() }
    }

    /// # Safety
    /// Preserve this slot's active member and initialization state until its
    /// phase transition completes; never exchange payloads with another phase.
    unsafe fn payload_mut<'s, I: crate::depot::raw::Compact>(
        slot: &'s mut Slot<'_, K, Q, P, Self, I>,
    ) -> &'s mut arena::raw::StagedValue<K, Q, P> {
        // SAFETY: the mutable Slot loan excludes all overlapping payload access.
        unsafe { (&mut *crate::depot::raw::StagedNode::of(slot).value.get()).assume_init_mut() }
    }
}

unsafe impl<K, Q, P> crate::depot::raw::StagedPhase<K, Q, P> for staged::Queued {
    type Value = Q;
    const QUEUED: bool = true;
    fn new(previous: links::Index) -> Self {
        Self(previous)
    }
    fn previous(&self) -> links::Index {
        self.0
    }
    fn value<'s, I: crate::depot::raw::Compact>(slot: &'s Slot<'_, K, Q, P, Self, I>) -> &'s Q {
        // SAFETY: the retained queued slot proves its active initialized member.
        unsafe { Self::payload(slot).queued_ref() }
    }
    fn value_mut<'s, I: crate::depot::raw::Compact>(
        slot: &'s mut Slot<'_, K, Q, P, Self, I>,
    ) -> &'s mut Q {
        // SAFETY: this exclusive queued loan changes only the active value.
        unsafe { Self::payload_mut(slot).queued_mut() }
    }
    unsafe fn take(payload: &mut arena::raw::StagedValue<K, Q, P>) -> Q {
        unsafe { payload.take_queued() }
    }
    unsafe fn write(payload: &mut arena::raw::StagedValue<K, Q, P>, value: Q) {
        payload.write_queued(value);
    }
    unsafe fn clear(payload: &mut arena::raw::StagedValue<K, Q, P>) {
        unsafe { payload.clear_queued() };
    }
}

unsafe impl<K, Q, P> crate::depot::raw::StagedPhase<K, Q, P> for staged::Parked {
    type Value = P;
    const QUEUED: bool = false;
    fn new(previous: links::Index) -> Self {
        Self(previous)
    }
    fn previous(&self) -> links::Index {
        self.0
    }
    fn value<'s, I: crate::depot::raw::Compact>(slot: &'s Slot<'_, K, Q, P, Self, I>) -> &'s P {
        // SAFETY: the retained parked slot proves its active initialized member.
        unsafe { Self::payload(slot).parked_ref() }
    }
    fn value_mut<'s, I: crate::depot::raw::Compact>(
        slot: &'s mut Slot<'_, K, Q, P, Self, I>,
    ) -> &'s mut P {
        // SAFETY: this exclusive parked loan changes only the active value.
        unsafe { Self::payload_mut(slot).parked_mut() }
    }
    unsafe fn take(payload: &mut arena::raw::StagedValue<K, Q, P>) -> P {
        unsafe { payload.take_parked() }
    }
    unsafe fn write(payload: &mut arena::raw::StagedValue<K, Q, P>, value: P) {
        payload.write_parked(value);
    }
    unsafe fn clear(payload: &mut arena::raw::StagedValue<K, Q, P>) {
        unsafe { payload.clear_parked() };
    }
}
