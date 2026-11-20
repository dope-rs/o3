use super::StagedNode;
use crate::depot::{
    queue::{
        indexed::fifo::staged::{self, Slot},
        links,
    },
    raw::Compact,
};

/// # Safety
/// Value is exactly Q for the queued chain and P for the parked chain.
/// The phase retains the supplied predecessor unchanged, without callbacks or owner reentry.
pub unsafe trait StagedPhase<Q, P>: Sized {
    type Value;
    const QUEUED: bool;
    fn new(previous: links::Index) -> Self;
    fn previous(&self) -> links::Index;
}

unsafe impl<Q, P> StagedPhase<Q, P> for staged::Queued {
    type Value = Q;
    const QUEUED: bool = true;

    fn new(previous: links::Index) -> Self {
        Self(previous)
    }

    fn previous(&self) -> links::Index {
        self.0
    }
}

unsafe impl<Q, P> StagedPhase<Q, P> for staged::Parked {
    type Value = P;
    const QUEUED: bool = false;

    fn new(previous: links::Index) -> Self {
        Self(previous)
    }

    fn previous(&self) -> links::Index {
        self.0
    }
}

impl<K, Q, P, S: StagedPhase<Q, P>, I: Compact> Slot<'_, K, Q, P, S, I> {
    pub fn key(&self) -> &K {
        // SAFETY: the occupied slot borrows the initialized key.
        unsafe { &*StagedNode::of(self).key_ptr() }
    }
}

impl<K, Q, P, S: StagedPhase<Q, P>, I: Compact> AsRef<S::Value> for Slot<'_, K, Q, P, S, I> {
    fn as_ref(&self) -> &S::Value {
        // SAFETY: S identifies the active field for the slot borrow.
        unsafe { &*StagedNode::of(self).value_ptr::<S>() }
    }
}

impl<K, Q, P, S: StagedPhase<Q, P>, I: Compact> AsMut<S::Value> for Slot<'_, K, Q, P, S, I> {
    fn as_mut(&mut self) -> &mut S::Value {
        // SAFETY: the mutable slot loan excludes all other access to its field.
        unsafe { &mut *StagedNode::of(self).value_ptr::<S>() }
    }
}
