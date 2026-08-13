use std::mem;

use crate::collections::slab::sealed::{self, core};

pub(in crate::collections::slab::sealed) struct Busy<
    'a,
    T,
    G: sealed::GenerationState,
    M: core::Mode,
    const PARTITIONS: usize,
> {
    core: &'a core::Core<T, G, M, PARTITIONS>,
    index: core::SlotIndex,
    generation: G,
    live: bool,
}

impl<'a, T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>
    Busy<'a, T, G, M, PARTITIONS>
{
    pub(in crate::collections::slab::sealed) fn take_committed(
        core: &'a core::Core<T, G, M, PARTITIONS>,
        ticket: core::Ticket<G>,
    ) -> Self {
        let slot = unsafe { core.slots.get_unchecked(ticket.index.get() as usize) };
        debug_assert!(
            slot.state.get() == core::State::Occupied && slot.generation.get() == ticket.generation
        );
        if M::REENTRANT {
            slot.state.set(core::State::Busy);
        }
        Self {
            core,
            index: ticket.index,
            generation: ticket.generation,
            live: true,
        }
    }

    pub(in crate::collections::slab::sealed) fn take(
        core: &'a core::Core<T, G, M, PARTITIONS>,
        index: u32,
    ) -> Option<Self> {
        let slot = core.slots.get(index as usize)?;
        let index = core::SlotIndex::new(index, core.slots.len())?;
        let state = if M::REENTRANT {
            slot.state.replace(core::State::Busy)
        } else {
            slot.state.get()
        };
        if state == core::State::Occupied {
            Some(Self {
                core,
                index,
                generation: slot.generation.get(),
                live: true,
            })
        } else {
            if M::REENTRANT {
                slot.state.set(state);
            }
            None
        }
    }

    pub(in crate::collections::slab::sealed) fn take_key(
        core: &'a core::Core<T, G, M, PARTITIONS>,
        index: u32,
        generation: G,
    ) -> Option<Self> {
        let busy = Self::take(core, index)?;
        (busy.generation == generation).then_some(busy)
    }

    pub(in crate::collections::slab::sealed) fn index(&self) -> u32 {
        self.index.get()
    }

    pub(in crate::collections::slab::sealed) fn generation(&self) -> G {
        self.generation
    }

    pub(in crate::collections::slab::sealed) fn value(&self) -> &T {
        unsafe { self.slot().value() }
    }

    pub(in crate::collections::slab::sealed) fn value_mut(&mut self) -> &mut T {
        unsafe {
            &mut *self
                .core
                .slots
                .get_unchecked(self.index.get() as usize)
                .value_ptr()
        }
    }

    fn slot(&self) -> &core::Slot<T, G> {
        unsafe { self.core.slots.get_unchecked(self.index.get() as usize) }
    }

    pub(in crate::collections::slab::sealed) fn commit_removal(mut self) -> (T, core::SlotIndex) {
        let next = self.generation.next();
        let value = unsafe { self.slot().take_value() };
        self.live = false;
        self.core.entries().remove_occupied(self.index);
        match next {
            Some(generation) => self.core.reservations().release(self.index, generation),
            None if self.core.recycle_generations() => {
                self.core.reservations().release(self.index, G::MIN)
            }
            None => self.slot().state.set(core::State::Retired),
        }
        (value, self.index)
    }
}

pub(super) struct Inserted<
    'a,
    T,
    G: sealed::GenerationState,
    M: core::Mode,
    const PARTITIONS: usize,
> {
    busy: mem::ManuallyDrop<Busy<'a, T, G, M, PARTITIONS>>,
    rollback: bool,
}

impl<'a, T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>
    Inserted<'a, T, G, M, PARTITIONS>
{
    pub(super) fn new(core: &'a core::Core<T, G, M, PARTITIONS>, ticket: core::Ticket<G>) -> Self {
        Self {
            busy: mem::ManuallyDrop::new(Busy::take_committed(core, ticket)),
            rollback: true,
        }
    }

    pub(super) fn value_mut(&mut self) -> &mut T {
        self.busy.value_mut()
    }

    pub(super) fn commit(mut self) {
        self.rollback = false;
        unsafe { mem::ManuallyDrop::drop(&mut self.busy) };
    }

    pub(super) fn rollback(mut self) -> T {
        self.rollback = false;
        unsafe { mem::ManuallyDrop::take(&mut self.busy) }
            .commit_removal()
            .0
    }
}

impl<T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize> Drop
    for Inserted<'_, T, G, M, PARTITIONS>
{
    fn drop(&mut self) {
        if self.rollback {
            let busy = unsafe { mem::ManuallyDrop::take(&mut self.busy) };
            drop(busy.commit_removal().0);
        }
    }
}

impl<T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize> Drop
    for Busy<'_, T, G, M, PARTITIONS>
{
    fn drop(&mut self) {
        if self.live && M::REENTRANT {
            self.slot().state.set(core::State::Occupied);
        }
    }
}
