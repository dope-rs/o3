use std::{marker, mem, pin, slice};

use crate::collections::{
    self,
    slab::{self, key},
};

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Occupied,
    Dropping,
    Retired,
}

pub(super) struct Slot<T, const MAX: u32> {
    value: mem::MaybeUninit<T>,
    generation: key::Generation<MAX>,
    next: u32,
    state: State,
}

impl<T, const MAX: u32> Slot<T, MAX> {
    pub(super) fn linked(index: usize, capacity: usize) -> Self {
        Self {
            value: mem::MaybeUninit::uninit(),
            generation: key::Generation::MIN,
            next: if index + 1 == capacity {
                NONE
            } else {
                index as u32 + 1
            },
            state: State::Free,
        }
    }

    fn vacant() -> Self {
        Self {
            value: mem::MaybeUninit::uninit(),
            generation: key::Generation::MIN,
            next: NONE,
            state: State::Free,
        }
    }
}

pub(super) trait Slots<T, const MAX: u32> {
    fn initialized(&self, initialized: u32) -> &[Slot<T, MAX>];
    fn initialized_mut(&mut self, initialized: u32) -> &mut [Slot<T, MAX>];
    fn capacity(&self) -> usize;
    fn initial_initialized(&self) -> u32;
    fn initial_free(&self) -> u32;
    fn initialize(&mut self, index: u32) -> bool;
}

impl<T, S, const MAX: u32> Slots<T, MAX> for S
where
    S: AsRef<[Slot<T, MAX>]> + AsMut<[Slot<T, MAX>]>,
{
    fn initialized(&self, initialized: u32) -> &[Slot<T, MAX>] {
        debug_assert_eq!(initialized as usize, self.as_ref().len());
        self.as_ref()
    }

    fn initialized_mut(&mut self, initialized: u32) -> &mut [Slot<T, MAX>] {
        debug_assert_eq!(initialized as usize, self.as_ref().len());
        self.as_mut()
    }

    fn capacity(&self) -> usize {
        self.as_ref().len()
    }

    fn initial_initialized(&self) -> u32 {
        self.as_ref().len() as u32
    }

    fn initial_free(&self) -> u32 {
        if self.as_ref().is_empty() { NONE } else { 0 }
    }

    fn initialize(&mut self, _index: u32) -> bool {
        false
    }
}

pub(super) struct Lazy<T, const MAX: u32> {
    slots: Box<[mem::MaybeUninit<Slot<T, MAX>>]>,
}

impl<T, const MAX: u32> Lazy<T, MAX> {
    pub(super) fn try_with_capacity(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            slots: collections::BoxUninitExt::try_box_uninit(capacity.get())?,
        })
    }
}

impl<T, const MAX: u32> Slots<T, MAX> for Lazy<T, MAX> {
    fn initialized(&self, initialized: u32) -> &[Slot<T, MAX>] {
        unsafe { slice::from_raw_parts(self.slots.as_ptr().cast(), initialized as usize) }
    }

    fn initialized_mut(&mut self, initialized: u32) -> &mut [Slot<T, MAX>] {
        unsafe { slice::from_raw_parts_mut(self.slots.as_mut_ptr().cast(), initialized as usize) }
    }

    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn initial_initialized(&self) -> u32 {
        0
    }

    fn initial_free(&self) -> u32 {
        NONE
    }

    fn initialize(&mut self, index: u32) -> bool {
        if index as usize == self.slots.len() {
            return false;
        }
        self.slots[index as usize].write(Slot::vacant());
        true
    }
}

pub(super) struct Core<T, Tag, S, const MAX: u32> {
    slots: S,
    free: u32,
    initialized: u32,
    len: usize,
    value: marker::PhantomData<fn(T)>,
    tag: marker::PhantomData<fn() -> Tag>,
    _thread: crate::ThreadBound,
}

struct Reclaim<T, Tag, S: Slots<T, MAX>, const MAX: u32> {
    core: *mut Core<T, Tag, S, MAX>,
    index: u32,
    armed: bool,
}

pub(super) struct Vacant<'a, T, Tag, S, const MAX: u32> {
    core: &'a mut Core<T, Tag, S, MAX>,
    index: u32,
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> Vacant<'_, T, Tag, S, MAX> {
    pub(super) fn insert(self, value: T) -> key::Handle<Tag, MAX> {
        let slot = unsafe {
            self.core
                .slots
                .initialized_mut(self.core.initialized)
                .get_unchecked_mut(self.index as usize)
        };
        debug_assert!(self.core.free == self.index && slot.state == State::Free);
        self.core.free = slot.next;
        slot.value.write(value);
        slot.next = NONE;
        slot.state = State::Occupied;
        self.core.len += 1;
        key::Handle::new(self.index, slot.generation)
    }
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> Reclaim<T, Tag, S, MAX> {
    fn finish(mut self) {
        self.armed = false;
        unsafe { (*self.core).release(self.index) };
    }
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> Drop for Reclaim<T, Tag, S, MAX> {
    fn drop(&mut self) {
        if self.armed {
            unsafe { (*self.core).release(self.index) };
        }
    }
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> Core<T, Tag, S, MAX> {
    pub(super) fn new(slots: S) -> Self {
        use crate::ThreadBound;
        let _ = key::Generation::<MAX>::MIN;
        assert!(
            slots.capacity() <= u32::MAX as usize,
            "pin slab capacity overflow"
        );
        let free = slots.initial_free();
        let initialized = slots.initial_initialized();
        Self {
            slots,
            free,
            initialized,
            len: 0,
            value: marker::PhantomData,
            tag: marker::PhantomData,
            _thread: ThreadBound::NEW,
        }
    }

    pub(super) fn insert(&mut self, value: T) -> Result<key::Handle<Tag, MAX>, T> {
        match self.vacant_entry() {
            Some(entry) => Ok(entry.insert(value)),
            None => Err(value),
        }
    }

    pub(super) fn vacant_entry(&mut self) -> Option<Vacant<'_, T, Tag, S, MAX>> {
        if self.free == NONE {
            if !self.slots.initialize(self.initialized) {
                return None;
            }
            self.free = self.initialized;
            self.initialized += 1;
        }
        Some(Vacant {
            index: self.free,
            core: self,
        })
    }

    pub(super) fn contains_parts(&self, parts: key::Parts<MAX>) -> bool {
        self.slot(parts).is_some()
    }

    pub(super) fn parts(&self, parts: key::Parts<MAX>) -> Option<pin::Pin<&T>> {
        let slot = self.slot(parts)?;
        Some(unsafe { pin::Pin::new_unchecked(slot.value.assume_init_ref()) })
    }

    pub(super) fn parts_mut(&mut self, parts: key::Parts<MAX>) -> Option<pin::Pin<&mut T>> {
        let slot = self
            .slots
            .initialized_mut(self.initialized)
            .get_mut(parts.index() as usize)?;
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return None;
        }
        Some(unsafe { pin::Pin::new_unchecked(slot.value.assume_init_mut()) })
    }

    pub(super) fn index_mut(
        &mut self,
        index: u32,
    ) -> Option<(key::Handle<Tag, MAX>, pin::Pin<&mut T>)> {
        let slot = self
            .slots
            .initialized_mut(self.initialized)
            .get_mut(index as usize)?;
        if slot.state != State::Occupied {
            return None;
        }
        let key = key::Handle::new(index, slot.generation);
        let value = unsafe { pin::Pin::new_unchecked(slot.value.assume_init_mut()) };
        Some((key, value))
    }

    fn remove_index(&mut self, index: u32) {
        let slot = unsafe {
            self.slots
                .initialized_mut(self.initialized)
                .get_unchecked_mut(index as usize)
        };
        debug_assert!(slot.state == State::Occupied);
        slot.state = State::Dropping;
        self.len -= 1;
        let reclaim = Reclaim {
            core: self,
            index,
            armed: true,
        };
        unsafe {
            self.slots
                .initialized_mut(self.initialized)
                .get_unchecked_mut(index as usize)
                .value
                .assume_init_drop()
        };
        reclaim.finish();
    }

    pub(super) fn remove_parts(&mut self, parts: key::Parts<MAX>) -> bool {
        let Some(slot) = self
            .slots
            .initialized(self.initialized)
            .get(parts.index() as usize)
        else {
            return false;
        };
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return false;
        }
        self.remove_index(parts.index());
        true
    }

    pub(super) fn remove_parts_with<R>(
        &mut self,
        parts: key::Parts<MAX>,
        use_value: impl for<'a> FnOnce(pin::Pin<&'a mut T>) -> R,
    ) -> Option<R> {
        let result = use_value(self.parts_mut(parts)?);
        self.remove_index(parts.index());
        Some(result)
    }

    pub(super) fn take_parts(&mut self, parts: key::Parts<MAX>) -> Option<T>
    where
        T: Unpin,
    {
        let slot = self
            .slots
            .initialized_mut(self.initialized)
            .get_mut(parts.index() as usize)?;
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return None;
        }
        slot.state = State::Dropping;
        self.len -= 1;
        let value = unsafe { slot.value.assume_init_read() };
        self.release(parts.index());
        Some(value)
    }

    fn slot(&self, parts: key::Parts<MAX>) -> Option<&Slot<T, MAX>> {
        let slot = self
            .slots
            .initialized(self.initialized)
            .get(parts.index() as usize)?;
        (slot.state == State::Occupied && slot.generation == parts.generation()).then_some(slot)
    }

    fn release(&mut self, index: u32) {
        let slot = unsafe {
            self.slots
                .initialized_mut(self.initialized)
                .get_unchecked_mut(index as usize)
        };
        let Some(generation) = slot.generation.checked_add(1) else {
            slot.state = State::Retired;
            slot.next = NONE;
            return;
        };
        slot.generation = generation;
        slot.next = self.free;
        slot.state = State::Free;
        self.free = index;
    }

    pub(super) fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) fn key(&self, index: u32) -> Option<key::Handle<Tag, MAX>> {
        let slot = self
            .slots
            .initialized(self.initialized)
            .get(index as usize)?;
        (slot.state == State::Occupied).then(|| key::Handle::new(index, slot.generation))
    }

    pub(super) fn clear(&mut self) {
        for slot in self.slots.initialized_mut(self.initialized) {
            if slot.state == State::Occupied {
                slot.state = State::Dropping;
                self.len -= 1;
                unsafe { slot.value.assume_init_drop() };
            }
        }
    }
}
