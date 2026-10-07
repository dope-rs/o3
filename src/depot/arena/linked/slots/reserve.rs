use std::{marker, mem};

use crate::depot::{
    self,
    arena::{linked, linked::slots},
    capacity,
};

/// Capacity retained by one input traversal, including its recycled slots.
pub struct Reserve<'a, T, Tag> {
    store: &'a slots::Store<T>,
    remaining: u32,
    domain: marker::PhantomData<fn(Tag) -> Tag>,
}

impl<'a, T, Tag> Reserve<'a, T, Tag> {
    pub(in crate::depot::arena::linked) const fn new(
        store: &'a slots::Store<T>,
        count: capacity::Count,
    ) -> Self {
        Self {
            store,
            remaining: count.raw(),
            domain: marker::PhantomData,
        }
    }

    /// Consumes one proved outstanding reservation without repeating admission.
    pub fn claim(
        proof: depot::raw::Proof<&mut Self, linked::Available>,
        value: T,
    ) -> slots::Live<T, &'a slots::Store<T>, Tag> {
        let reserve = proof.into_inner();
        reserve.remaining = reserve.remaining.wrapping_sub(1);
        unsafe { slots::Vacant::claim(reserve.store) }.insert(value)
    }

    /// Returns this exact store's vacant slot to the traversal's reserved capacity.
    pub fn recycle(proof: Vacancy<'_, 'a, Self, T, Tag>) {
        let (reserve, slot) = proof.into_inner();
        slot.recycle();
        reserve.remaining = reserve.remaining.wrapping_add(1);
    }
}

impl<T, Tag> Drop for Reserve<'_, T, Tag> {
    fn drop(&mut self) {
        self.store.refund(self.remaining);
    }
}

const _: () = assert!(mem::size_of::<Reserve<'static, u64, ()>>() == 2 * mem::size_of::<usize>());

type Vacancy<'s, 'a, S, T, Tag> =
    depot::raw::Proof<(&'s mut S, slots::Vacant<T, &'a slots::Store<T>, Tag>), linked::Same>;
