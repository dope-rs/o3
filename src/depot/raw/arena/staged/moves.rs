use std::num::NonZero;

use super::{StagedNode, StagedPhase};
use crate::depot::{
    queue::indexed::fifo::staged::{Parked, Phase, Queued, Slot},
    raw::Compact,
};

type Removal<'a, K, Q, P, I, V> = (Slot<'a, K, Q, P, (), I>, (K, V));

impl<'a, K, Q, P, S, I: Compact> Slot<'a, K, Q, P, S, I> {
    /// # Safety
    /// This slot's node is detached and owns the fully initialized destination fields.
    unsafe fn link<D: StagedPhase<Q, P>>(&self, node: &StagedNode<K, Q, P>) -> D {
        let state = self.owner.nodes.state();
        let previous;
        let next;
        if D::QUEUED {
            previous = match state.tail.get() {
                Some(tail) => {
                    unsafe { StagedNode::at(self.owner, tail) }
                        .next
                        .set(Some(self.index));
                    tail
                }
                None => {
                    state.head.set(Some(self.index));
                    self.index
                }
            };
            next = None;
            state.tail.set(Some(self.index));
        } else {
            previous = self.index;
            next = state.parked.replace(Some(self.index));
            if let Some(head) = next {
                unsafe { StagedNode::at(self.owner, head) }
                    .previous
                    .set(self.index);
            }
        }
        node.previous.set(previous);
        node.next.set(next);
        node.phase.set(if D::QUEUED {
            Phase::Queued
        } else {
            Phase::Parked
        });
        D::new(previous)
    }
}

impl<'a, K, Q, P, I: Compact> Slot<'a, K, Q, P, (), I> {
    fn install(self, key: K, value: Q) -> Queued {
        let node = StagedNode::of(&self);
        let count = &self.owner.nodes.state().occupied;
        // SAFETY: this vacancy owns both uninitialized fields and proves count < capacity.
        unsafe {
            node.key_ptr().write(key);
            node.value_ptr::<Queued>().write(value);
            count.set(count.get() + 1);
            self.link::<Queued>(node)
        }
    }

    pub fn push_back(self, key: K, value: Q) -> Slot<'a, K, Q, P, Queued, I> {
        Slot {
            owner: self.owner,
            index: self.index,
            loan: self.loan,
            phase: self.install(key, value),
        }
    }

    pub fn insert(self, key: K, value: Q) {
        self.install(key, value);
    }
}

impl<'a, K, Q, P, S: StagedPhase<Q, P>, I: Compact> Slot<'a, K, Q, P, S, I> {
    /// # Safety
    /// Detach this slot's node from its current chain and finish its transition under the loan.
    unsafe fn unlink(&self, node: &StagedNode<K, Q, P>) {
        let state = self.owner.nodes.state();
        let next = node.next.get();
        let previous = self.phase.previous();
        let first = previous == self.index;
        if first {
            let head = if S::QUEUED {
                &state.head
            } else {
                &state.parked
            };
            head.set(next);
        } else {
            unsafe { StagedNode::at(self.owner, previous) }
                .next
                .set(next);
        }
        if let Some(next) = next {
            unsafe { StagedNode::at(self.owner, next) }
                .previous
                .set(if first { next } else { previous });
        }
        if S::QUEUED && next.is_none() {
            state.tail.set(if first { None } else { Some(previous) });
        }
    }

    fn replace<D: StagedPhase<Q, P>>(self, value: D::Value) -> (Slot<'a, K, Q, P, D, I>, S::Value) {
        let node = StagedNode::of(&self);
        // SAFETY: the sole slot loan moves S out and D in at the same address.
        // The key and occupied count stay live throughout the chain transfer.
        unsafe {
            let prior = node.value_ptr::<S>().read();
            node.value_ptr::<D>().write(value);
            self.unlink(node);
            let phase = self.link::<D>(node);
            let slot = Slot {
                owner: self.owner,
                index: self.index,
                phase,
                loan: self.loan,
            };
            (slot, prior)
        }
    }

    /// # Safety
    /// Consume this slot and move or drop the detached fields before lending the vacancy.
    unsafe fn retire(&self) -> &'a StagedNode<K, Q, P> {
        let node = StagedNode::of(self);
        let count = &self.owner.nodes.state().occupied;
        // SAFETY: this occupied slot contributes exactly one to the count.
        let occupied = unsafe { NonZero::new_unchecked(count.get()) };
        unsafe { self.unlink(node) };
        node.phase.set(Phase::Vacant);
        count.set(occupied.get() - 1);
        node
    }

    pub fn remove(self) -> Removal<'a, K, Q, P, I, S::Value> {
        // SAFETY: the consumed slot moves both detached fields before returning the vacancy.
        let fields = unsafe {
            let node = self.retire();
            (node.key_ptr().read(), node.value_ptr::<S>().read())
        };
        let slot = Slot {
            owner: self.owner,
            index: self.index,
            phase: (),
            loan: self.loan,
        };
        (slot, fields)
    }

    pub fn take(self) -> (K, S::Value) {
        self.remove().1
    }

    pub fn clear(self) {
        // SAFETY: the consumed slot owns both fields until their destruction completes.
        unsafe { self.retire().drop_fields::<S>() };
    }
}

impl<'a, K, Q, P, I: Compact> Slot<'a, K, Q, P, Queued, I> {
    pub fn park(self, value: P) -> (Slot<'a, K, Q, P, Parked, I>, Q) {
        self.replace(value)
    }

    pub fn set_parked(self, value: P) -> Q {
        self.park(value).1
    }
}

impl<'a, K, Q, P, I: Compact> Slot<'a, K, Q, P, Parked, I> {
    pub fn resume(self, value: Q) -> (Slot<'a, K, Q, P, Queued, I>, P) {
        self.replace(value)
    }
}
