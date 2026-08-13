use std::marker;

use crate::collections::slab::{key, sealed};

pub struct VacantEntry<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
> {
    pub(in crate::collections::slab::sealed) slab:
        &'a sealed::Exclusive<T, Tag, MAX, RECYCLE, PARTITIONS>,
    pub(in crate::collections::slab::sealed) ticket:
        Option<sealed::core::Ticket<key::Generation<MAX>>>,
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    VacantEntry<'a, T, Tag, MAX, RECYCLE, PARTITIONS>
{
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        let ticket = unsafe { self.ticket.unwrap_unchecked() };
        key::Handle::new(ticket.index.get(), ticket.generation)
    }

    pub fn insert(mut self, value: T) -> key::Handle<Tag, MAX> {
        let ticket = unsafe { self.ticket.unwrap_unchecked() };
        let key = key::Handle::new(ticket.index.get(), ticket.generation);
        let ticket = unsafe { self.ticket.take().unwrap_unchecked() };
        self.slab.core.reservations().commit(ticket, value);
        key
    }

    pub fn insert_occupied(
        mut self,
        value: T,
    ) -> sealed::OccupiedEntry<'a, T, Tag, MAX, RECYCLE, PARTITIONS> {
        let ticket = unsafe { self.ticket.take().unwrap_unchecked() };
        let slab = self.slab;
        slab.core.reservations().commit(ticket, value);
        sealed::OccupiedEntry {
            busy: sealed::core::Busy::take_committed(&slab.core, ticket),
            tag: marker::PhantomData,
        }
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize> Drop
    for VacantEntry<'_, T, Tag, MAX, RECYCLE, PARTITIONS>
{
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.slab.core.reservations().rollback(ticket);
        }
    }
}
