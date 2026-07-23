use super::{Mode, SlabCore, Slot, SlotIndex, State};
use crate::collections::slab::GenerationState;

pub(super) struct Busy<'a, T, G: GenerationState, M: Mode> {
    core: &'a SlabCore<T, G, M>,
    index: SlotIndex,
    generation: G,
    live: bool,
}

impl<'a, T, G: GenerationState, M: Mode> Busy<'a, T, G, M> {
    pub(super) fn take(core: &'a SlabCore<T, G, M>, index: u32) -> Option<Self> {
        let slot = core.slots.get(index as usize)?;
        let index = SlotIndex::new(index, core.slots.len())?;
        let state = if M::REENTRANT {
            slot.state.replace(State::Busy)
        } else {
            slot.state.get()
        };
        if state == State::Occupied {
            Some(Self {
                core,
                index,
                generation: slot.generation.get(),
                live: true,
            })
        } else {
            slot.state.set(state);
            None
        }
    }

    pub(super) fn take_key(core: &'a SlabCore<T, G, M>, index: u32, generation: G) -> Option<Self> {
        let busy = Self::take(core, index)?;
        (busy.generation == generation).then_some(busy)
    }

    pub(super) fn generation(&self) -> G {
        self.generation
    }

    pub(super) fn value_mut(&mut self) -> &mut T {
        unsafe {
            &mut *self
                .core
                .slots
                .get_unchecked(self.index.get() as usize)
                .value_ptr()
        }
    }

    fn slot(&self) -> &Slot<T, G> {
        unsafe { self.core.slots.get_unchecked(self.index.get() as usize) }
    }

    pub(super) fn commit_removal(mut self) -> (T, SlotIndex) {
        let next = self.generation.next();
        let value = unsafe { self.slot().take_value() };
        self.live = false;
        self.core.remove_occupied(self.index);
        match next {
            Some(generation) => self.core.release(self.index, generation),
            None => self.slot().state.set(State::Retired),
        }
        (value, self.index)
    }
}

impl<T, G: GenerationState, M: Mode> Drop for Busy<'_, T, G, M> {
    fn drop(&mut self) {
        if self.live && M::REENTRANT {
            self.slot().state.set(State::Occupied);
        }
    }
}
