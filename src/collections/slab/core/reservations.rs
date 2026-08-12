use std::hint;

use crate::collections::slab::{self, core};

pub(in crate::collections::slab) struct Reservations<'a, T, G: slab::GenerationState, M: core::Mode>
{
    core: &'a core::Core<T, G, M>,
}

impl<'a, T, G: slab::GenerationState, M: core::Mode> Reservations<'a, T, G, M> {
    pub(super) fn new(core: &'a core::Core<T, G, M>) -> Self {
        Self { core }
    }

    pub(in crate::collections::slab) fn take_free(&self) -> Option<core::Ticket<G>> {
        let raw = self.core.free.get();
        if raw == core::NONE {
            return None;
        }
        Some(unsafe { self.take_free_raw(raw) })
    }

    unsafe fn take_free_raw(&self, raw: u32) -> core::Ticket<G> {
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
        self.core.free.set(next);
        slot.state.set(core::State::Reserved);
        core::Ticket { index, generation }
    }

    pub(in crate::collections::slab) fn take_index(&self, index: u32) -> Option<core::Ticket<G>> {
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
            debug_assert_eq!(self.core.free.get(), index.get());
            self.core.free.set(next);
        } else {
            self.set_free_next(prev, next);
        }
        if next != core::NONE {
            self.set_free_prev(next, prev);
        }
    }

    pub(super) fn release(&self, index: core::SlotIndex, generation: G) {
        let head = self.core.free.replace(index.get());
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

    pub(in crate::collections::slab) fn commit(&self, ticket: core::Ticket<G>, value: T) {
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

    pub(in crate::collections::slab) fn rollback(&self, ticket: core::Ticket<G>) {
        let slot = reserved_slot(self.core, ticket);
        match ticket.generation.next() {
            Some(generation) => self.release(ticket.index, generation),
            None if self.core.recycle_generations() => self.release(ticket.index, G::MIN),
            None => slot.state.set(core::State::Retired),
        }
    }
}

fn free_slot<T, G: slab::GenerationState, M: core::Mode>(
    core: &core::Core<T, G, M>,
    index: u32,
) -> &core::Slot<T, G> {
    let slot = unsafe { core.slots.get_unchecked(index as usize) };
    if slot.state.get() != core::State::Free {
        unsafe { hint::unreachable_unchecked() }
    }
    slot
}

fn reserved_slot<T, G: slab::GenerationState, M: core::Mode>(
    core: &core::Core<T, G, M>,
    ticket: core::Ticket<G>,
) -> &core::Slot<T, G> {
    let slot = unsafe { core.slots.get_unchecked(ticket.index.get() as usize) };
    debug_assert!(
        slot.state.get() == core::State::Reserved && slot.generation.get() == ticket.generation
    );
    slot
}
