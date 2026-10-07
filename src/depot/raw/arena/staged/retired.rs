use std::marker;

use crate::depot::queue::indexed::fifo::staged;

/// Owns the initialized payload after its topology and count have been retired.
#[must_use]
pub(in crate::depot) struct Retired<'a, K, Q, P, S, I: crate::depot::raw::Compact> {
    slot: staged::Slot<'a, K, Q, P, (), I>,
    phase: marker::PhantomData<S>,
}

impl<'a, K, Q, P, S: crate::depot::raw::StagedPhase<K, Q, P>, I: crate::depot::raw::Compact>
    Retired<'a, K, Q, P, S, I>
{
    pub(in crate::depot) fn new(slot: staged::Slot<'a, K, Q, P, S, I>) -> Self {
        let occupied = S::occupied(&slot);
        // SAFETY: the consumed occupied phase retains its exact reciprocal slot;
        // this private owner keeps the payload inaccessible until it is consumed.
        let slot = unsafe { S::unlink(slot) }.release(occupied);
        Self {
            slot,
            phase: marker::PhantomData,
        }
    }

    pub(in crate::depot) fn into_parts(self) -> Removal<'a, K, Q, P, I, S::Value> {
        // SAFETY: retirement changed metadata only; this owner retains exactly
        // one initialized key and active value, which are moved once together.
        let payload = unsafe {
            (&mut *crate::depot::raw::StagedNode::of(&self.slot).value.get()).assume_init_mut()
        };
        let value = unsafe { S::take(payload) };
        let key = unsafe { payload.take_key() };
        (self.slot, (key, value))
    }

    pub(in crate::depot) fn destroy(self) {
        // SAFETY: metadata was retired before any destructor can run; the
        // exclusive owner still contains this phase's initialized key and value.
        unsafe {
            S::clear(
                (&mut *crate::depot::raw::StagedNode::of(&self.slot).value.get()).assume_init_mut(),
            )
        };
    }
}

/// Retirement returns the detached reservation beside its exact initialized payload.
type Removal<'a, K, Q, P, I, V> = (staged::Slot<'a, K, Q, P, (), I>, (K, V));
