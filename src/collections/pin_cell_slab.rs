use std::cell::{Cell, UnsafeCell};
use std::marker::{PhantomData, PhantomPinned};
use std::mem::{MaybeUninit, forget};
use std::pin::Pin;

use crate::collections::slab::GenerationState;
use crate::collections::{ClearGuard, SlabGeneration, SlabKey, SlabKeyParts};
use crate::marker::ThreadBound;

const NONE: u32 = u32::MAX;
const OCCUPIED: u32 = u32::MAX - 1;
const BORROWED: u32 = u32::MAX - 2;
const DROPPING: u32 = u32::MAX - 3;
const RETIRED: u32 = u32::MAX - 4;

struct Slot<T, const MAX: u32> {
    value: UnsafeCell<MaybeUninit<T>>,
    generation: Cell<SlabGeneration<MAX>>,
    link: Cell<u32>,
}

impl<T, const MAX: u32> Slot<T, MAX> {
    fn new(index: usize, capacity: usize) -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            generation: Cell::new(SlabGeneration::MIN),
            link: Cell::new(if index + 1 == capacity {
                NONE
            } else {
                index as u32 + 1
            }),
        }
    }
}

#[must_use]
pub struct PinCellSlabVacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    slab: &'a PinCellSlab<T, Tag, MAX>,
    index: u32,
}

impl<T, Tag, const MAX: u32> PinCellSlabVacantEntry<'_, T, Tag, MAX> {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn insert(self, value: T) -> SlabKey<Tag, MAX> {
        let key = self.slab.commit(self.index, value);
        forget(self);
        key
    }
}

impl<T, Tag, const MAX: u32> Drop for PinCellSlabVacantEntry<'_, T, Tag, MAX> {
    fn drop(&mut self) {
        self.slab.rollback(self.index);
    }
}

#[must_use]
pub struct PinCellSlabOccupiedEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    slab: &'a PinCellSlab<T, Tag, MAX>,
    key: SlabKey<Tag, MAX>,
}

impl<T, Tag, const MAX: u32> PinCellSlabOccupiedEntry<'_, T, Tag, MAX> {
    pub fn key(&self) -> SlabKey<Tag, MAX> {
        self.key
    }

    pub fn index(&self) -> u32 {
        self.key.index()
    }

    pub fn generation(&self) -> SlabGeneration<MAX> {
        self.key.generation()
    }

    pub fn parts(&self) -> SlabKeyParts<MAX> {
        self.key.parts()
    }

    pub fn as_pin_mut(&mut self) -> Pin<&mut T> {
        let slot = unsafe { self.slab.slots.get_unchecked(self.index() as usize) };
        debug_assert!(slot.link.get() == BORROWED && slot.generation.get() == self.generation());
        unsafe { Pin::new_unchecked((&mut *slot.value.get()).assume_init_mut()) }
    }

    pub fn remove(self) {
        self.slab.remove_index(self.index());
    }
}

impl<T, Tag, const MAX: u32> Drop for PinCellSlabOccupiedEntry<'_, T, Tag, MAX> {
    fn drop(&mut self) {
        let slot = unsafe { self.slab.slots.get_unchecked(self.index() as usize) };
        if slot.link.get() == BORROWED {
            slot.link.set(OCCUPIED);
        }
    }
}

struct Reclaim<'a, T, Tag, const MAX: u32> {
    slab: &'a PinCellSlab<T, Tag, MAX>,
    index: u32,
    armed: bool,
}

impl<T, Tag, const MAX: u32> Reclaim<'_, T, Tag, MAX> {
    fn finish(mut self) {
        self.armed = false;
        self.slab.release(self.index);
    }
}

impl<T, Tag, const MAX: u32> Drop for Reclaim<'_, T, Tag, MAX> {
    fn drop(&mut self) {
        if self.armed {
            self.slab.release(self.index);
        }
    }
}

pub struct PinCellSlab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    slots: Box<[Slot<T, MAX>]>,
    free: Cell<u32>,
    len: Cell<usize>,
    tag: PhantomData<fn() -> Tag>,
    _thread: ThreadBound,
    _pin: PhantomPinned,
}

impl<T, Tag, const MAX: u32> PinCellSlab<T, Tag, MAX> {
    pub fn with_capacity(capacity: usize) -> Self {
        let _ = SlabGeneration::<MAX>::MIN;
        assert!(
            capacity <= RETIRED as usize,
            "pin cell slab capacity overflow"
        );
        Self {
            slots: (0..capacity)
                .map(|index| Slot::new(index, capacity))
                .collect(),
            free: Cell::new(if capacity == 0 { NONE } else { 0 }),
            len: Cell::new(0),
            tag: PhantomData,
            _thread: ThreadBound::NEW,
            _pin: PhantomPinned,
        }
    }

    pub fn insert(self: Pin<&Self>, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        match self.vacant_entry() {
            Some(entry) => Ok(entry.insert(value)),
            None => Err(value),
        }
    }

    pub fn vacant_entry(self: Pin<&Self>) -> Option<PinCellSlabVacantEntry<'_, T, Tag, MAX>> {
        let this = self.get_ref();
        let index = this.free.get();
        if index == NONE {
            return None;
        }
        let slot = unsafe { this.slots.get_unchecked(index as usize) };
        this.free.set(slot.link.get());
        Some(PinCellSlabVacantEntry { slab: this, index })
    }

    pub fn entry(
        self: Pin<&Self>,
        key: SlabKey<Tag, MAX>,
    ) -> Option<PinCellSlabOccupiedEntry<'_, T, Tag, MAX>> {
        self.entry_parts(key.parts())
    }

    pub fn entry_parts(
        self: Pin<&Self>,
        parts: SlabKeyParts<MAX>,
    ) -> Option<PinCellSlabOccupiedEntry<'_, T, Tag, MAX>> {
        let this = self.get_ref();
        let slot = this.slots.get(parts.index() as usize)?;
        if slot.link.get() != OCCUPIED || slot.generation.get() != parts.generation() {
            return None;
        }
        slot.link.set(BORROWED);
        Some(PinCellSlabOccupiedEntry {
            slab: this,
            key: SlabKey::from_parts(parts),
        })
    }

    pub fn contains_key(&self, key: SlabKey<Tag, MAX>) -> bool {
        self.contains_parts(key.parts())
    }

    pub fn contains_parts(&self, parts: SlabKeyParts<MAX>) -> bool {
        self.slots.get(parts.index() as usize).is_some_and(|slot| {
            matches!(slot.link.get(), OCCUPIED | BORROWED)
                && slot.generation.get() == parts.generation()
        })
    }

    pub fn update<R>(
        self: Pin<&Self>,
        key: SlabKey<Tag, MAX>,
        f: impl FnOnce(Pin<&mut T>) -> R,
    ) -> Option<R> {
        self.update_parts(key.parts(), f)
    }

    pub fn update_parts<R>(
        self: Pin<&Self>,
        parts: SlabKeyParts<MAX>,
        f: impl FnOnce(Pin<&mut T>) -> R,
    ) -> Option<R> {
        let mut entry = self.entry_parts(parts)?;
        Some(f(entry.as_pin_mut()))
    }

    pub fn remove(self: Pin<&Self>, key: SlabKey<Tag, MAX>) -> bool {
        self.remove_parts(key.parts())
    }

    pub fn remove_if(
        self: Pin<&Self>,
        key: SlabKey<Tag, MAX>,
        predicate: impl FnOnce(Pin<&mut T>) -> bool,
    ) -> bool {
        self.remove_parts_if(key.parts(), predicate)
    }

    pub fn remove_parts(self: Pin<&Self>, parts: SlabKeyParts<MAX>) -> bool {
        let Some(entry) = self.entry_parts(parts) else {
            return false;
        };
        entry.remove();
        true
    }

    pub fn remove_parts_if(
        self: Pin<&Self>,
        parts: SlabKeyParts<MAX>,
        predicate: impl FnOnce(Pin<&mut T>) -> bool,
    ) -> bool {
        let Some(mut entry) = self.entry_parts(parts) else {
            return false;
        };
        if !predicate(entry.as_pin_mut()) {
            return false;
        }
        entry.remove();
        true
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub fn len(&self) -> usize {
        self.len.get()
    }

    pub fn is_empty(&self) -> bool {
        self.len.get() == 0
    }

    pub fn is_full(&self) -> bool {
        self.free.get() == NONE
    }

    fn commit(&self, index: u32, value: T) -> SlabKey<Tag, MAX> {
        let slot = unsafe { self.slots.get_unchecked(index as usize) };
        debug_assert!(!matches!(
            slot.link.get(),
            OCCUPIED | BORROWED | DROPPING | RETIRED
        ));
        let key = SlabKey::new(index, slot.generation.get());
        unsafe { (*slot.value.get()).write(value) };
        slot.link.set(OCCUPIED);
        self.len.set(self.len.get() + 1);
        key
    }

    fn rollback(&self, index: u32) {
        let slot = unsafe { self.slots.get_unchecked(index as usize) };
        debug_assert!(!matches!(
            slot.link.get(),
            OCCUPIED | BORROWED | DROPPING | RETIRED
        ));
        slot.link.set(self.free.get());
        self.free.set(index);
    }

    fn remove_index(&self, index: u32) {
        let slot = unsafe { self.slots.get_unchecked(index as usize) };
        debug_assert!(slot.link.get() == BORROWED);
        slot.link.set(DROPPING);
        self.len.set(self.len.get() - 1);
        let reclaim = Reclaim {
            slab: self,
            index,
            armed: true,
        };
        unsafe { (&mut *slot.value.get()).assume_init_drop() };
        reclaim.finish();
    }

    fn release(&self, index: u32) {
        let slot = unsafe { self.slots.get_unchecked(index as usize) };
        let Some(generation) = slot.generation.get().next() else {
            slot.link.set(RETIRED);
            return;
        };
        slot.generation.set(generation);
        slot.link.set(self.free.get());
        self.free.set(index);
    }

    fn clear(&mut self) {
        for index in 0..self.slots.len() {
            if self.slots[index].link.get() == OCCUPIED {
                let slot = &self.slots[index];
                slot.link.set(DROPPING);
                self.len.set(self.len.get() - 1);
                let reclaim = Reclaim {
                    slab: self,
                    index: index as u32,
                    armed: true,
                };
                unsafe { (&mut *slot.value.get()).assume_init_drop() };
                drop(reclaim);
            }
        }
    }
}

impl<T, Tag, const MAX: u32> Drop for PinCellSlab<T, Tag, MAX> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}
