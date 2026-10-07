use std::mem;

use front::Front;

use crate::depot::{arena::linked::slots, capacity, queue::pool};

pub(super) mod front;
mod raw;
pub(super) mod vacancy;
pub(super) mod view;

/// Exclusive access to one exact FIFO lane and its state.
pub struct Lane<'a, T, S = (), const TRACK: bool = true> {
    pub(in crate::depot::queue::pool) store: &'a slots::Store<T>,
    pub(in crate::depot::queue::pool) slot: &'a mut crate::depot::raw::QueueSlot<S>,
}

impl<'a, T, S, const TRACK: bool> Lane<'a, T, S, TRACK> {
    pub(in crate::depot) const fn new(
        store: &'a slots::Store<T>,
        slot: &'a mut crate::depot::raw::QueueSlot<S>,
    ) -> Self {
        Self { store, slot }
    }

    /// Consumes the exact lane and its original unary insertion entitlement.
    pub fn insert<I>(source: I) -> &'a mut S
    where
        I: crate::depot::raw::PoolInsert<'a, TRACK, Item = T, State = S>,
    {
        // SAFETY: the admitted source retains this lane's unused slot entitlement.
        let (lane, item) = unsafe { source.into_parts() };
        let claim =
            unsafe { slots::Vacant::insert_one(slots::Borrow::<_, TRACK>(lane.store), item) };
        // SAFETY: the claim belongs to this retained lane's exact store.
        unsafe { lane.slot.queue.append(claim) };
        &mut lane.slot.state
    }

    pub const fn count(&self) -> capacity::Count {
        self.slot.queue.count()
    }

    pub const fn is_empty(&self) -> bool {
        self.slot.queue.is_empty()
    }

    pub const fn state(&self) -> &S {
        &self.slot.state
    }

    pub const fn state_mut(&mut self) -> &mut S {
        &mut self.slot.state
    }

    pub fn into_state(self) -> &'a mut S {
        &mut self.slot.state
    }

    /// Reborrows this admitted lane without repeating its owner lookup.
    pub fn borrow_mut(&mut self) -> Lane<'_, T, S, TRACK> {
        Lane::new(self.store, self.slot)
    }

    pub fn front_entry(self) -> Option<Front<'a, T, S, TRACK>> {
        let index = self.slot.queue.front_index()?;
        Some(Front::new(self, index))
    }

    pub fn front_mut(&mut self) -> Option<Front<'_, T, S, TRACK>> {
        self.borrow_mut().front_entry()
    }

    pub fn pop_front(&mut self) -> Option<T> {
        Some(self.front_mut()?.pop())
    }

    /// Removes every value, returning each slot before destroying its payload.
    pub fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }
}

impl<'a, T, S> Lane<'a, T, S> {
    /// Reserves one slot and retains the disjoint state loan on both outcomes.
    pub fn reserve(self) -> (pool::Vacancy<'a, T>, &'a mut S) {
        let vacant = match self.store.vacant() {
            Some(claim) => pool::Vacancy::Ready(unsafe { claim.bind(&mut self.slot.queue) }),
            None => pool::Vacancy::Exhausted,
        };
        (vacant, &mut self.slot.state)
    }

    /// Reserves one exact slot until insertion or cancellation.
    pub fn vacant(&mut self) -> Option<slots::Vacant<T, slots::Bound<'_, T>>> {
        let claim = self.store.vacant()?;
        Some(unsafe { claim.bind(&mut self.slot.queue) })
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        let Some(vacant) = self.vacant() else {
            return Err(value);
        };
        vacant.append(value);
        Ok(())
    }

    /// Admits one complete input, then inserts its exact owners without capacity rechecks.
    pub fn try_extend<I>(&mut self, source: I) -> Result<I::Output, I::Error>
    where
        I: crate::depot::raw::Start<T, Item = T, Context = ()>,
    {
        let (input, ()) = slots::Input::try_new(self.store, source)?.start();
        Ok(input.run(|claim, value| {
            unsafe { claim.bind(&mut self.slot.queue) }.append(value);
        }))
    }
}

const _: () = {
    assert!(mem::size_of::<Lane<'static, u64>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<crate::depot::raw::QueueSlot<()>>() == 3 * mem::size_of::<u32>());
};
