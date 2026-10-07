use std::mem;

use crate::{
    depot::{arena::linked::slots, index::Initialized, queue::pool},
    mem::{Lease, lease},
};

/// A nonempty front bound to one exclusively borrowed lane.
pub struct Front<'a, T, S, const TRACK: bool = true> {
    lane: pool::Lane<'a, T, S, TRACK>,
    index: slots::Index,
}

impl<'a, T, S, const TRACK: bool> Front<'a, T, S, TRACK> {
    /// Reuses this exact lane's retained live head identity without another lookup.
    pub fn from_source(
        source: crate::depot::raw::Proof<
            (pool::Lane<'a, T, S, TRACK>, slots::Index),
            Initialized<pool::Head>,
        >,
    ) -> Self {
        let (lane, index) = source.into_inner();
        Self { lane, index }
    }

    /// Retains the admitted lane and its compact current head identity.
    pub fn into_parts(self) -> (pool::Lane<'a, T, S, TRACK>, slots::Index) {
        (self.lane, self.index)
    }

    pub(in crate::depot::queue::pool::lane) const fn new(
        lane: pool::Lane<'a, T, S, TRACK>,
        index: slots::Index,
    ) -> Self {
        Self { lane, index }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.parts_mut().0
    }

    /// Borrows the initialized front and its disjoint lane state together.
    pub fn parts_mut(&mut self) -> (&mut T, &mut S) {
        let value = unsafe { &mut *self.lane.store.value_ptr(self.index) };
        (value, &mut self.lane.slot.state)
    }

    /// Consumes this front and retains its original lane for continued processing.
    pub fn take(self) -> (T, pool::Lane<'a, T, S, TRACK>) {
        // SAFETY: Front retains this initialized front and its exact lane policy.
        let value = unsafe {
            self.lane
                .slot
                .queue
                .pop::<_, TRACK>(self.lane.store, self.index)
        };
        (value, self.lane)
    }

    pub fn pop(self) -> T {
        self.take().0
    }
}

impl<'a, T, S> Front<'a, T, S> {
    /// Lends the removed head and returns its original lane after the affine slot is settled.
    pub fn remove_with<R>(
        mut self,
        remove: impl for<'b> FnOnce(Lease<slots::Vacant<T, slots::Bound<'b, T>>, lease::Open>) -> R,
    ) -> (pool::Lane<'a, T, S>, R) {
        let result = remove(Front::new(self.lane.borrow_mut(), self.index).remove());
        (self.lane, result)
    }

    /// Retains this exact slot while its front owner is transformed or returned.
    pub fn remove(self) -> Lease<slots::Vacant<T, slots::Bound<'a, T>>, lease::Open> {
        let lane = self.lane;
        // SAFETY: Front owns this initialized slot and its exact counted store and lane.
        let (value, claim) = unsafe { lane.slot.queue.take(lane.store, self.index) };
        Lease::open(unsafe { claim.bind(&mut lane.slot.queue) }, value)
    }
}

const _: () = {
    assert!(mem::size_of::<Front<'static, u64, ()>>() == 3 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Option<Front<'static, u64, ()>>>()
            == mem::size_of::<Front<'static, u64, ()>>()
    );
    assert!(
        mem::size_of::<(u64, pool::Lane<'static, u64>)>()
            == mem::size_of::<Front<'static, u64, ()>>()
    );
};
