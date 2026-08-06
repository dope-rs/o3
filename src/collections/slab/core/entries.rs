use crate::collections::slab::{
    GenerationState,
    core::{Mode, NONE, SlabCore, SlotIndex, State, guards::Busy},
};

pub(in crate::collections::slab) trait Entries<T, G: GenerationState, M: Mode> {
    fn get(&self, index: u32, generation: G) -> Option<&T>;
    fn get_mut(&mut self, index: u32, generation: G) -> Option<&mut T>;
    fn remove(&self, index: u32, generation: G) -> Option<(T, SlotIndex)>;
    fn remove_with<R>(
        &self,
        index: u32,
        generation: G,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)>;
    fn remove_index(&self, index: u32) -> Option<(T, SlotIndex)>;
    fn remove_index_with<R>(
        &self,
        index: u32,
        f: impl FnOnce(&mut T, G) -> Option<R>,
    ) -> Option<(T, R, SlotIndex)>;
    fn index(&self, index: u32) -> Option<(&T, G)>;
    fn index_mut(&mut self, index: u32) -> Option<(&mut T, G)>;
    fn generation(&self, index: u32) -> Option<G>;
    fn occupied_at(&self, position: usize) -> Option<(u32, G)>;
    fn values<'a>(&'a self) -> impl Iterator<Item = &'a T>
    where
        T: 'a;
    fn values_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a;
    fn clear(&mut self);
    fn update<R>(&self, index: u32, generation: G, f: impl FnOnce(&mut T) -> R) -> Option<R>;
    fn remove_occupied(&self, index: SlotIndex);
}

impl<T, G: GenerationState, M: Mode> Entries<T, G, M> for SlabCore<T, G, M> {
    fn get(&self, index: u32, generation: G) -> Option<&T> {
        let slot = self.slots.get(index as usize)?;
        if slot.state.get() == State::Occupied && slot.generation.get() == generation {
            Some(unsafe { slot.value() })
        } else {
            None
        }
    }

    fn get_mut(&mut self, index: u32, generation: G) -> Option<&mut T> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.state.get() == State::Occupied && slot.generation.get() == generation {
            Some(unsafe { &mut *slot.value_ptr() })
        } else {
            None
        }
    }

    fn remove(&self, index: u32, generation: G) -> Option<(T, SlotIndex)> {
        Some(Busy::take_key(self, index, generation)?.commit_removal())
    }

    fn remove_with<R>(
        &self,
        index: u32,
        generation: G,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        let mut busy = Busy::take_key(self, index, generation)?;
        let result = f(busy.value_mut())?;
        let (value, _) = busy.commit_removal();
        Some((value, result))
    }

    fn remove_index(&self, index: u32) -> Option<(T, SlotIndex)> {
        Some(Busy::take(self, index)?.commit_removal())
    }

    fn remove_index_with<R>(
        &self,
        index: u32,
        f: impl FnOnce(&mut T, G) -> Option<R>,
    ) -> Option<(T, R, SlotIndex)> {
        let mut busy = Busy::take(self, index)?;
        let generation = busy.generation();
        let result = f(busy.value_mut(), generation)?;
        let (value, index) = busy.commit_removal();
        Some((value, result, index))
    }

    fn index(&self, index: u32) -> Option<(&T, G)> {
        let slot = self.slots.get(index as usize)?;
        if slot.state.get() == State::Occupied {
            Some((unsafe { slot.value() }, slot.generation.get()))
        } else {
            None
        }
    }

    fn index_mut(&mut self, index: u32) -> Option<(&mut T, G)> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.state.get() == State::Occupied {
            let generation = slot.generation.get();
            Some((unsafe { &mut *slot.value_ptr() }, generation))
        } else {
            None
        }
    }

    fn generation(&self, index: u32) -> Option<G> {
        let slot = self.slots.get(index as usize)?;
        (slot.state.get() == State::Occupied).then(|| slot.generation.get())
    }

    fn occupied_at(&self, position: usize) -> Option<(u32, G)> {
        if position >= self.len() {
            return None;
        }
        let index = self.occupied.get(position)?.get();
        let slot = self.slots.get(index as usize)?;
        (slot.state.get() == State::Occupied && slot.position.get() as usize == position)
            .then(|| (index, slot.generation.get()))
    }

    fn values<'a>(&'a self) -> impl Iterator<Item = &'a T>
    where
        T: 'a,
    {
        (0..self.len()).map(|position| {
            let index = unsafe { self.occupied.get_unchecked(position) }.get();
            let slot = unsafe { self.slots.get_unchecked(index as usize) };
            debug_assert!(slot.state.get() == State::Occupied);
            unsafe { slot.value() }
        })
    }

    fn values_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a,
    {
        let len = self.len();
        let slots = self.slots.as_mut_ptr();
        let occupied = &self.occupied;
        (0..len).map(move |position| {
            let index = unsafe { occupied.get_unchecked(position) }.get();
            let slot = unsafe { &mut *slots.add(index as usize) };
            debug_assert!(slot.state.get() == State::Occupied);
            unsafe { &mut *slot.value_ptr() }
        })
    }

    fn clear(&mut self) {
        while self.len.get() != 0 {
            let position = self.len.get() - 1;
            let index = unsafe { self.occupied.get_unchecked(position as usize) }.get();
            drop(self.remove_index(index).map(|(value, _)| value));
        }
    }

    fn update<R>(&self, index: u32, generation: G, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut busy = Busy::take_key(self, index, generation)?;
        Some(f(busy.value_mut()))
    }

    fn remove_occupied(&self, index: SlotIndex) {
        let slot = unsafe { self.slots.get_unchecked(index.get() as usize) };
        let position = slot.position.replace(NONE);
        let last_position = self.len.get() - 1;
        let last_index = unsafe { self.occupied.get_unchecked(last_position as usize) }.get();
        unsafe { self.occupied.get_unchecked(position as usize) }.set(last_index);
        unsafe { self.occupied.get_unchecked(last_position as usize) }.set(NONE);
        if last_index != index.get() {
            unsafe { self.slots.get_unchecked(last_index as usize) }
                .position
                .set(position);
        }
        self.len.set(last_position);
    }
}
