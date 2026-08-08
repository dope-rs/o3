use crate::collections::slab::{
    self,
    core::{self, guards},
};

#[derive(Clone, Copy)]
pub(in crate::collections::slab) struct Entries<'a, T, G: slab::GenerationState, M: core::Mode> {
    core: &'a core::Core<T, G, M>,
}

impl<'a, T, G: slab::GenerationState, M: core::Mode> Entries<'a, T, G, M> {
    pub(super) fn new(core: &'a core::Core<T, G, M>) -> Self {
        Self { core }
    }

    pub(in crate::collections::slab) fn get(self, index: u32, generation: G) -> Option<&'a T> {
        let slot = self.core.slots.get(index as usize)?;
        if slot.state.get() == core::State::Occupied && slot.generation.get() == generation {
            Some(unsafe { slot.value() })
        } else {
            None
        }
    }

    pub(in crate::collections::slab) fn remove(
        self,
        index: u32,
        generation: G,
    ) -> Option<(T, core::SlotIndex)> {
        Some(guards::Busy::take_key(self.core, index, generation)?.commit_removal())
    }

    pub(in crate::collections::slab) fn remove_with<R>(
        self,
        index: u32,
        generation: G,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        let mut busy = guards::Busy::take_key(self.core, index, generation)?;
        let result = f(busy.value_mut())?;
        let (value, _) = busy.commit_removal();
        Some((value, result))
    }

    pub(in crate::collections::slab) fn remove_index(
        self,
        index: u32,
    ) -> Option<(T, core::SlotIndex)> {
        Some(guards::Busy::take(self.core, index)?.commit_removal())
    }

    pub(in crate::collections::slab) fn remove_index_with<R>(
        self,
        index: u32,
        f: impl FnOnce(&mut T, G) -> Option<R>,
    ) -> Option<(T, R, core::SlotIndex)> {
        let mut busy = guards::Busy::take(self.core, index)?;
        let generation = busy.generation();
        let result = f(busy.value_mut(), generation)?;
        let (value, index) = busy.commit_removal();
        Some((value, result, index))
    }

    pub(in crate::collections::slab) fn index(self, index: u32) -> Option<(&'a T, G)> {
        let slot = self.core.slots.get(index as usize)?;
        if slot.state.get() == core::State::Occupied {
            Some((unsafe { slot.value() }, slot.generation.get()))
        } else {
            None
        }
    }

    pub(in crate::collections::slab) fn generation(self, index: u32) -> Option<G> {
        let slot = self.core.slots.get(index as usize)?;
        (slot.state.get() == core::State::Occupied).then(|| slot.generation.get())
    }

    pub(in crate::collections::slab) fn occupied_at(self, position: usize) -> Option<(u32, G)> {
        if position >= self.core.len() {
            return None;
        }
        let index = self.core.occupied.get(position)?.get();
        let slot = self.core.slots.get(index as usize)?;
        (slot.state.get() == core::State::Occupied && slot.position.get() as usize == position)
            .then(|| (index, slot.generation.get()))
    }

    pub(in crate::collections::slab) fn values(self) -> impl Iterator<Item = &'a T> + 'a {
        (0..self.core.len()).map(|position| {
            let index = unsafe { self.core.occupied.get_unchecked(position) }.get();
            let slot = unsafe { self.core.slots.get_unchecked(index as usize) };
            debug_assert!(slot.state.get() == core::State::Occupied);
            unsafe { slot.value() }
        })
    }

    pub(super) fn clear(self) {
        let core = self.core;
        while core.len.get() != 0 {
            let position = core.len.get() - 1;
            let index = unsafe { core.occupied.get_unchecked(position as usize) }.get();
            drop(Self::new(core).remove_index(index).map(|(value, _)| value));
        }
    }

    pub(in crate::collections::slab) fn update<R>(
        self,
        index: u32,
        generation: G,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        let mut busy = guards::Busy::take_key(self.core, index, generation)?;
        Some(f(busy.value_mut()))
    }

    pub(super) fn remove_occupied(self, index: core::SlotIndex) {
        let slot = unsafe { self.core.slots.get_unchecked(index.get() as usize) };
        let position = slot.position.replace(core::NONE);
        let last_position = self.core.len.get() - 1;
        let last_index = unsafe { self.core.occupied.get_unchecked(last_position as usize) }.get();
        unsafe { self.core.occupied.get_unchecked(position as usize) }.set(last_index);
        unsafe { self.core.occupied.get_unchecked(last_position as usize) }.set(core::NONE);
        if last_index != index.get() {
            unsafe { self.core.slots.get_unchecked(last_index as usize) }
                .position
                .set(position);
        }
        self.core.len.set(last_position);
    }
}
