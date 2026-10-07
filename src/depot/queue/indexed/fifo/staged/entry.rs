use std::marker;

use crate::depot::queue::indexed::fifo::staged;

/// Exclusive phase proof for one indexed staged slot.
pub enum Entry<'queue, Key, Queued, Parked, I: crate::depot::raw::Compact = u32> {
    Vacant(staged::Slot<'queue, Key, Queued, Parked, (), I>),
    Queued(staged::Slot<'queue, Key, Queued, Parked, staged::Queued, I>),
    Parked(staged::Slot<'queue, Key, Queued, Parked, staged::Parked, I>),
}

impl<'a, K, Q, P, I: crate::depot::raw::Compact> Entry<'a, K, Q, P, I> {
    pub fn from_source(
        source: crate::depot::raw::Proof<staged::Member<'a, K, Q, P, I>, staged::Exclusive>,
    ) -> Self {
        let (owner, index) = source.into_inner().into_parts();

        let node = unsafe { crate::depot::raw::StagedNode::at(owner, index) };
        match node.phase.get() {
            staged::Phase::Queued => staged::Entry::Queued(staged::Slot {
                owner,
                index,
                phase: staged::Queued(node.previous.get()),
                loan: marker::PhantomData,
            }),
            staged::Phase::Parked => staged::Entry::Parked(staged::Slot {
                owner,
                index,
                phase: staged::Parked(node.previous.get()),
                loan: marker::PhantomData,
            }),
            staged::Phase::Vacant => staged::Entry::Vacant(staged::Slot {
                owner,
                index,
                phase: (),
                loan: marker::PhantomData,
            }),
        }
    }
}
