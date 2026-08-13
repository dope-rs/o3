use std::{hint, ops};

use crate::collections::slab::sealed::{self, core};

pub(in crate::collections::slab::sealed) struct Reservations<
    'a,
    T,
    G: sealed::GenerationState,
    M: core::Mode,
    const PARTITIONS: usize,
> {
    core: &'a core::Core<T, G, M, PARTITIONS>,
}

impl<'a, T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>
    Reservations<'a, T, G, M, PARTITIONS>
{
    pub(super) fn new(core: &'a core::Core<T, G, M, PARTITIONS>) -> Self {
        Self { core }
    }

    pub(in crate::collections::slab::sealed) fn take_free(&self) -> Option<core::Ticket<G>> {
        if let Some(ticket) = self.take_partition(0) {
            return Some(ticket);
        }
        if PARTITIONS == 2 {
            return self.take_partition(1);
        }
        None
    }

    pub(in crate::collections::slab::sealed) fn take_range(
        &self,
        mut range: ops::Range<u32>,
    ) -> Option<core::Ticket<G>> {
        let capacity = self.core.capacity() as u32;
        if range == (0..capacity) {
            return self.take_free();
        }
        if PARTITIONS == 2 {
            let boundary = capacity.div_ceil(2);
            if range == (0..boundary) {
                return self.take_partition(0);
            }
            if range == (boundary..capacity) {
                return self.take_partition(1);
            }
        }
        range.find_map(|index| self.take_index(index))
    }

    fn take_partition(&self, partition: usize) -> Option<core::Ticket<G>> {
        let raw = self.core.free.get(partition)?.get();
        (raw != core::NONE).then(|| unsafe { self.take_free_raw(raw, partition) })
    }

    unsafe fn take_free_raw(&self, raw: u32, partition: usize) -> core::Ticket<G> {
        let index = core::SlotIndex(raw);
        let slot = unsafe { self.core.slots.get_unchecked(index.get() as usize) };
        if slot.state.get() != core::State::Free {
            unsafe { hint::unreachable_unchecked() }
        }
        let generation = slot.generation.get();
        let core::Links { next, prev } = unsafe { slot.links() };
        debug_assert_eq!(prev, core::NONE);
        if next != core::NONE {
            let next = unsafe { self.core.slots.get_unchecked(next as usize) };
            if next.state.get() != core::State::Free {
                unsafe { hint::unreachable_unchecked() }
            }
            let core::Links { next: link, .. } = unsafe { next.links() };
            unsafe {
                next.set_links(core::Links {
                    next: link,
                    prev: core::NONE,
                })
            };
        }
        unsafe { self.core.free.get_unchecked(partition) }.set(next);
        slot.state.set(core::State::Reserved);
        core::Ticket { index, generation }
    }

    pub(in crate::collections::slab::sealed) fn take_index(
        &self,
        index: u32,
    ) -> Option<core::Ticket<G>> {
        let slot = self.core.slots.get(index as usize)?;
        let index = core::SlotIndex::new(index, self.core.slots.len())?;
        if slot.state.get() != core::State::Free {
            return None;
        }
        let generation = slot.generation.get();
        let core::Links { next, prev } = unsafe { slot.links() };
        self.unlink(index, prev, next);
        slot.state.set(core::State::Reserved);
        Some(core::Ticket { index, generation })
    }

    fn unlink(&self, index: core::SlotIndex, prev: u32, next: u32) {
        if prev == core::NONE {
            let partition = self.core.partition(index);
            let head = unsafe { self.core.free.get_unchecked(partition) };
            debug_assert_eq!(head.get(), index.get());
            head.set(next);
        } else {
            self.set_free_next(prev, next);
        }
        if next != core::NONE {
            self.set_free_prev(next, prev);
        }
    }

    pub(super) fn release(&self, index: core::SlotIndex, generation: G) {
        let partition = self.core.partition(index);
        let head = unsafe { self.core.free.get_unchecked(partition) }.replace(index.get());
        let slot = unsafe { self.core.slots.get_unchecked(index.get() as usize) };
        slot.generation.set(generation);
        unsafe {
            slot.set_links(core::Links {
                next: head,
                prev: core::NONE,
            })
        };
        slot.state.set(core::State::Free);
        if head != core::NONE {
            self.set_free_prev(head, index.get());
        }
    }

    fn set_free_next(&self, index: u32, next: u32) {
        let slot = free_slot(self.core, index);
        let core::Links { prev, .. } = unsafe { slot.links() };
        unsafe { slot.set_links(core::Links { next, prev }) };
    }

    pub(super) fn set_free_prev(&self, index: u32, prev: u32) {
        let slot = free_slot(self.core, index);
        let core::Links { next, .. } = unsafe { slot.links() };
        unsafe { slot.set_links(core::Links { next, prev }) };
    }

    pub(in crate::collections::slab::sealed) fn commit(&self, ticket: core::Ticket<G>, value: T) {
        let slot = reserved_slot(self.core, ticket);
        unsafe { slot.write_value(value) };
        self.commit_initialized(ticket);
    }

    fn commit_initialized(&self, ticket: core::Ticket<G>) {
        let slot = reserved_slot(self.core, ticket);
        let position = self.core.len.get();
        unsafe { self.core.occupied.get_unchecked(position as usize) }.set(ticket.index.get());
        slot.position.set(position);
        slot.state.set(core::State::Occupied);
        self.core.len.set(position + 1);
    }

    pub(in crate::collections::slab::sealed) fn rollback(&self, ticket: core::Ticket<G>) {
        let slot = reserved_slot(self.core, ticket);
        match ticket.generation.next() {
            Some(generation) => self.release(ticket.index, generation),
            None if self.core.recycle_generations() => self.release(ticket.index, G::MIN),
            None => slot.state.set(core::State::Retired),
        }
    }
}

fn free_slot<T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>(
    core: &core::Core<T, G, M, PARTITIONS>,
    index: u32,
) -> &core::Slot<T, G> {
    let slot = unsafe { core.slots.get_unchecked(index as usize) };
    if slot.state.get() != core::State::Free {
        unsafe { hint::unreachable_unchecked() }
    }
    slot
}

fn reserved_slot<T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>(
    core: &core::Core<T, G, M, PARTITIONS>,
    ticket: core::Ticket<G>,
) -> &core::Slot<T, G> {
    let slot = unsafe { core.slots.get_unchecked(ticket.index.get() as usize) };
    debug_assert!(
        slot.state.get() == core::State::Reserved && slot.generation.get() == ticket.generation
    );
    slot
}
