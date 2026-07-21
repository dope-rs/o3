use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::pin::Pin;

use crate::collections::slab::GenerationState;
use crate::collections::{ClearGuard, SlabGeneration, SlabKey, SlabKeyParts};
use crate::marker::ThreadBound;

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

struct CoreOccupiedEntry<'a, T, Tag, S: Slots<T, MAX>, const MAX: u32> {
    core: &'a mut Core<T, Tag, S, MAX>,
    key: SlabKey<Tag, MAX>,
}

struct CoreVacantEntry<'a, T, Tag, S: Slots<T, MAX>, const MAX: u32> {
    core: &'a mut Core<T, Tag, S, MAX>,
    index: u32,
}

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> CoreVacantEntry<'_, T, Tag, S, MAX> {
    fn index(&self) -> u32 {
        self.index
    }

    fn key(&self) -> SlabKey<Tag, MAX> {
        let slot = unsafe {
            self.core
                .slots
                .as_slice()
                .get_unchecked(self.index as usize)
        };
        debug_assert!(slot.state == State::Free);
        SlabKey::new(self.index, slot.generation)
    }

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

impl<T, Tag, S: Slots<T, MAX>, const MAX: u32> CoreOccupiedEntry<'_, T, Tag, S, MAX> {
    fn as_pin_mut(&mut self) -> Pin<&mut T> {
        let slot = unsafe {
            self.core
                .slots
                .as_mut_slice()
                .get_unchecked_mut(self.key.index() as usize)
        };
        debug_assert!(slot.state == State::Occupied && slot.generation == self.key.generation());
        unsafe { Pin::new_unchecked(slot.value.assume_init_mut()) }
    }

    fn remove(self) {
        self.core.remove_index(self.key.index());
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

    fn get_parts(&self, parts: SlabKeyParts<MAX>) -> Option<Pin<&T>> {
        let slot = self.slot(parts)?;
        Some(unsafe { Pin::new_unchecked(slot.value.assume_init_ref()) })
    }

    fn get_parts_mut(&mut self, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        let slot = self.slots.as_mut_slice().get_mut(parts.index() as usize)?;
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return None;
        }
        Some(unsafe { Pin::new_unchecked(slot.value.assume_init_mut()) })
    }

    fn entry_parts(
        &mut self,
        parts: SlabKeyParts<MAX>,
    ) -> Option<CoreOccupiedEntry<'_, T, Tag, S, MAX>> {
        let slot = self.slots.as_slice().get(parts.index() as usize)?;
        if slot.state != State::Occupied || slot.generation != parts.generation() {
            return None;
        }
        Some(CoreOccupiedEntry {
            core: self,
            key: SlabKey::from_parts(parts),
        })
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
        let Some(entry) = self.entry_parts(parts) else {
            return false;
        };
        entry.remove();
        true
    }

    fn take(&mut self, key: SlabKey<Tag, MAX>) -> Option<T>
    where
        T: Unpin,
    {
        let index = key.index();
        let slot = self.slots.as_mut_slice().get_mut(index as usize)?;
        if slot.state != State::Occupied || slot.generation != key.generation() {
            return None;
        }
        slot.state = State::Dropping;
        self.len -= 1;
        let value = unsafe { slot.value.assume_init_read() };
        self.release(index);
        Some(value)
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

    fn len(&self) -> usize {
        self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn is_full(&self) -> bool {
        self.free == NONE
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

macro_rules! impl_common {
    () => {
        pub fn contains_key(&self, key: SlabKey<Tag, MAX>) -> bool {
            self.contains_parts(key.parts())
        }

        pub fn contains_parts(&self, parts: SlabKeyParts<MAX>) -> bool {
            self.core.contains_parts(parts)
        }

        pub fn capacity(&self) -> usize {
            self.core.capacity()
        }

        pub fn len(&self) -> usize {
            self.core.len()
        }

        pub fn is_empty(&self) -> bool {
            self.core.is_empty()
        }

        pub fn is_full(&self) -> bool {
            self.core.is_full()
        }

        pub fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
            self.core.key(index)
        }
    };
}

macro_rules! impl_occupied_entry {
    () => {
        pub fn key(&self) -> SlabKey<Tag, MAX> {
            self.entry.key
        }

        pub fn index(&self) -> u32 {
            self.key().index()
        }

        pub fn generation(&self) -> SlabGeneration<MAX> {
            self.key().generation()
        }

        pub fn parts(&self) -> SlabKeyParts<MAX> {
            self.key().parts()
        }

        pub fn as_pin_mut(&mut self) -> Pin<&mut T> {
            self.entry.as_pin_mut()
        }

        pub fn remove(self) {
            self.entry.remove();
        }
    };
}

macro_rules! impl_vacant_entry {
    () => {
        pub fn index(&self) -> u32 {
            self.entry.index()
        }

        pub fn key(&self) -> SlabKey<Tag, MAX> {
            self.entry.key()
        }

        pub fn insert(self, value: T) -> SlabKey<Tag, MAX> {
            self.entry.insert(value)
        }
    };
}

mod fixed;

pub use fixed::{FixedPinSlab, FixedPinSlabOccupiedEntry, FixedPinSlabVacantEntry};

pub struct PinSlab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: Core<T, Tag, Box<[Slot<T, MAX>]>, MAX>,
}

#[must_use]
pub struct PinSlabOccupiedEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: CoreOccupiedEntry<'a, T, Tag, Box<[Slot<T, MAX>]>, MAX>,
}

#[must_use]
pub struct PinSlabVacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: CoreVacantEntry<'a, T, Tag, Box<[Slot<T, MAX>]>, MAX>,
}

impl<T, Tag, const MAX: u32> PinSlabOccupiedEntry<'_, T, Tag, MAX> {
    impl_occupied_entry!();
}

impl<T, Tag, const MAX: u32> PinSlabVacantEntry<'_, T, Tag, MAX> {
    impl_vacant_entry!();
}

impl<T, Tag, const MAX: u32> PinSlab<T, Tag, MAX> {
    pub fn with_capacity(capacity: usize) -> Self {
        validate_capacity(capacity);
        let slots = (0..capacity)
            .map(|index| Slot::new(index, capacity))
            .collect();
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

    impl_common!();

    pub fn get(&self, key: SlabKey<Tag, MAX>) -> Option<Pin<&T>> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(&self, parts: SlabKeyParts<MAX>) -> Option<Pin<&T>> {
        self.core.get_parts(parts)
    }

    pub fn get_mut(&mut self, key: SlabKey<Tag, MAX>) -> Option<Pin<&mut T>> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(&mut self, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        self.core.get_parts_mut(parts)
    }

    pub fn entry(
        &mut self,
        key: SlabKey<Tag, MAX>,
    ) -> Option<PinSlabOccupiedEntry<'_, T, Tag, MAX>> {
        self.entry_parts(key.parts())
    }

    pub fn entry_parts(
        &mut self,
        parts: SlabKeyParts<MAX>,
    ) -> Option<PinSlabOccupiedEntry<'_, T, Tag, MAX>> {
        Some(PinSlabOccupiedEntry {
            entry: self.core.entry_parts(parts)?,
        })
    }

    pub fn remove(&mut self, key: SlabKey<Tag, MAX>) -> bool {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&mut self, parts: SlabKeyParts<MAX>) -> bool {
        self.core.remove_parts(parts)
    }

    pub fn take(&mut self, key: SlabKey<Tag, MAX>) -> Option<T>
    where
        T: Unpin,
    {
        self.core.take(key)
    }
}
