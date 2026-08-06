use std::{marker::PhantomData, mem::MaybeUninit, pin::Pin};

use crate::{
    ThreadBound,
    collections::{
        ClearGuard,
        slab::{
            GenerationState, SlabCapacity,
            key::{SlabGeneration, SlabKey, SlabKeyParts},
        },
    },
};

const NONE: u32 = u32::MAX;

fn validate_capacity(capacity: usize) {
    assert!(capacity <= u32::MAX as usize, "pin slab capacity overflow");
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Occupied,
    Dropping,
    Retired,
}

struct Slot<T, const MAX: u32> {
    value: MaybeUninit<T>,
    generation: SlabGeneration<MAX>,
    next: u32,
    state: State,
}

impl<T, const MAX: u32> Slot<T, MAX> {
    fn new(index: usize, capacity: usize) -> Self {
        Self {
            value: MaybeUninit::uninit(),
            generation: SlabGeneration::MIN,
            next: if index + 1 == capacity {
                NONE
            } else {
                index as u32 + 1
            },
            state: State::Free,
        }
    }
}

trait Slots<T, const MAX: u32> {
    fn as_slice(&self) -> &[Slot<T, MAX>];
    fn as_mut_slice(&mut self) -> &mut [Slot<T, MAX>];
}

impl<T, S, const MAX: u32> Slots<T, MAX> for S
where
    S: AsRef<[Slot<T, MAX>]> + AsMut<[Slot<T, MAX>]>,
{
    fn as_slice(&self) -> &[Slot<T, MAX>] {
        self.as_ref()
    }

    fn as_mut_slice(&mut self) -> &mut [Slot<T, MAX>] {
        self.as_mut()
    }
}

struct Core<T, Tag, S: Slots<T, MAX>, const MAX: u32> {
    slots: S,
    free: u32,
    len: usize,
    value: PhantomData<fn(T)>,
    tag: PhantomData<fn() -> Tag>,
    _thread: ThreadBound,
}

struct Reclaim<T, Tag, S: Slots<T, MAX>, const MAX: u32> {
    core: *mut Core<T, Tag, S, MAX>,
    index: u32,
    armed: bool,
}

struct CoreVacantEntry<'a, T, Tag, S: Slots<T, MAX>, const MAX: u32> {
    core: &'a mut Core<T, Tag, S, MAX>,
    index: u32,
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> CoreVacantEntry<'_, T, Tag, S, MAX> {
    fn insert(self, value: T) -> SlabKey<Tag, MAX> {
        let slot = unsafe {
            self.core
                .slots
                .as_mut_slice()
                .get_unchecked_mut(self.index as usize)
        };
        debug_assert!(self.core.free == self.index && slot.state == State::Free);
        self.core.free = slot.next;
        slot.value.write(value);
        slot.next = NONE;
        slot.state = State::Occupied;
        self.core.len += 1;
        SlabKey::new(self.index, slot.generation)
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
    fn new(slots: S) -> Self {
        let _ = SlabGeneration::<MAX>::MIN;
        validate_capacity(slots.as_slice().len());
        let free = if slots.as_slice().is_empty() { NONE } else { 0 };
        Self {
            slots,
            free,
            len: 0,
            value: PhantomData,
            tag: PhantomData,
            _thread: ThreadBound::NEW,
        }
    }

    fn insert(&mut self, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        match self.vacant_entry() {
            Some(entry) => Ok(entry.insert(value)),
            None => Err(value),
        }
    }

    fn vacant_entry(&mut self) -> Option<CoreVacantEntry<'_, T, Tag, S, MAX>> {
        let index = self.free;
        (index != NONE).then_some(CoreVacantEntry { core: self, index })
    }

    fn contains_parts(&self, parts: SlabKeyParts<MAX>) -> bool {
        self.slot(parts).is_some()
    }

    fn parts(&self, parts: SlabKeyParts<MAX>) -> Option<Pin<&T>> {
        let slot = self.slot(parts)?;
        Some(unsafe { Pin::new_unchecked(slot.value.assume_init_ref()) })
    }

    fn parts_mut(&mut self, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        let slot = self.slots.as_mut_slice().get_mut(parts.index() as usize)?;
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return None;
        }
        Some(unsafe { Pin::new_unchecked(slot.value.assume_init_mut()) })
    }

    fn remove_index(&mut self, index: u32) {
        let slot = unsafe { self.slots.as_mut_slice().get_unchecked_mut(index as usize) };
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
                .as_mut_slice()
                .get_unchecked_mut(index as usize)
                .value
                .assume_init_drop()
        };
        reclaim.finish();
    }

    fn remove_parts(&mut self, parts: SlabKeyParts<MAX>) -> bool {
        let Some(slot) = self.slots.as_slice().get(parts.index() as usize) else {
            return false;
        };
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return false;
        }
        self.remove_index(parts.index());
        true
    }

    fn slot(&self, parts: SlabKeyParts<MAX>) -> Option<&Slot<T, MAX>> {
        let slot = self.slots.as_slice().get(parts.index() as usize)?;
        (slot.state == State::Occupied && slot.generation == parts.generation()).then_some(slot)
    }

    fn release(&mut self, index: u32) {
        let slot = unsafe { self.slots.as_mut_slice().get_unchecked_mut(index as usize) };
        let Some(generation) = slot.generation.next() else {
            slot.state = State::Retired;
            slot.next = NONE;
            return;
        };
        slot.generation = generation;
        slot.next = self.free;
        slot.state = State::Free;
        self.free = index;
    }

    fn capacity(&self) -> usize {
        self.slots.as_slice().len()
    }

    fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
        let slot = self.slots.as_slice().get(index as usize)?;
        (slot.state == State::Occupied).then(|| SlabKey::new(index, slot.generation))
    }

    fn clear(&mut self) {
        for slot in self.slots.as_mut_slice() {
            if slot.state == State::Occupied {
                slot.state = State::Dropping;
                self.len -= 1;
                unsafe { slot.value.assume_init_drop() };
            }
        }
    }
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> Drop for Core<T, Tag, S, MAX> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}

pub mod fixed;

pub struct PinSlab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: Core<T, Tag, Box<[Slot<T, MAX>]>, MAX>,
}

#[must_use]
pub struct PinSlabVacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: CoreVacantEntry<'a, T, Tag, Box<[Slot<T, MAX>]>, MAX>,
}

impl<T, Tag, const MAX: u32> PinSlabVacantEntry<'_, T, Tag, MAX> {
    pub fn insert(self, value: T) -> SlabKey<Tag, MAX> {
        self.entry.insert(value)
    }
}

impl<T, Tag, const MAX: u32> PinSlab<T, Tag, MAX> {
    pub fn with_capacity(capacity: SlabCapacity) -> Self {
        let raw_capacity = capacity.get();
        let slots =
            capacity.collect_box((0..raw_capacity).map(|index| Slot::new(index, raw_capacity)));
        Self {
            core: Core::new(slots),
        }
    }

    pub fn insert(&mut self, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        self.core.insert(value)
    }

    pub fn vacant_entry(&mut self) -> Option<PinSlabVacantEntry<'_, T, Tag, MAX>> {
        Some(PinSlabVacantEntry {
            entry: self.core.vacant_entry()?,
        })
    }

    pub fn contains_parts(&self, parts: SlabKeyParts<MAX>) -> bool {
        self.core.contains_parts(parts)
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn get(&self, key: SlabKey<Tag, MAX>) -> Option<Pin<&T>> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(&self, parts: SlabKeyParts<MAX>) -> Option<Pin<&T>> {
        self.core.parts(parts)
    }

    pub fn get_mut(&mut self, key: SlabKey<Tag, MAX>) -> Option<Pin<&mut T>> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(&mut self, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        self.core.parts_mut(parts)
    }

    pub fn remove(&mut self, key: SlabKey<Tag, MAX>) -> bool {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&mut self, parts: SlabKeyParts<MAX>) -> bool {
        self.core.remove_parts(parts)
    }
}
