use std::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;

use crate::collections::BoxSliceGrowth;
use crate::collections::ClearGuard;
use crate::marker::ThreadBound;

const NONE: u32 = u32::MAX;

struct Slot<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    prev: Cell<u32>,
    next: Cell<u32>,
}

impl<T> Slot<T> {
    fn vacant(index: u32) -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            prev: Cell::new(index),
            next: Cell::new(NONE),
        }
    }
}

#[derive(Clone, Copy)]
struct State {
    head: u32,
    tail: u32,
    len: usize,
}

impl State {
    const EMPTY: Self = Self {
        head: NONE,
        tail: NONE,
        len: 0,
    };
}

struct SlotQueueCore<T> {
    entries: Box<[Slot<T>]>,
    state: Cell<State>,
    _thread: ThreadBound,
}

impl<T> SlotQueueCore<T> {
    fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "slot queue capacity overflow"
        );
        Self {
            entries: (0..capacity as u32).map(Slot::vacant).collect(),
            state: Cell::new(State::EMPTY),
            _thread: ThreadBound::NEW,
        }
    }

    fn push_back(&self, index: usize, value: T) -> Result<(), T> {
        if !self.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.push_back_unchecked(index, value) };
        Ok(())
    }

    fn push_front(&self, index: usize, value: T) -> Result<(), T> {
        if !self.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.push_front_unchecked(index, value) };
        Ok(())
    }

    fn refresh_back(&self, index: usize, value: T) -> Result<Option<T>, T> {
        let Some(entry) = self.entries.get(index) else {
            return Err(value);
        };
        let previous =
            (entry.prev.get() != index as u32).then(|| unsafe { self.remove_unchecked(index) });
        unsafe { self.push_back_unchecked(index, value) };
        Ok(previous)
    }

    /// # Safety
    /// `index` is in bounds and vacant.
    unsafe fn push_front_unchecked(&self, index: usize, value: T) {
        debug_assert!(self.is_vacant(index));
        let entry = unsafe { self.entries.get_unchecked(index) };
        unsafe { (*entry.value.get()).write(value) };
        unsafe { self.link_front(index) };
    }

    /// # Safety
    /// `index` is in bounds and vacant.
    unsafe fn push_back_unchecked(&self, index: usize, value: T) {
        debug_assert!(self.is_vacant(index));
        let entry = unsafe { self.entries.get_unchecked(index) };
        unsafe { (*entry.value.get()).write(value) };
        unsafe { self.link_back(index) };
    }

    fn is_vacant(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.prev.get() == index as u32)
    }

    unsafe fn link_front(&self, index: usize) {
        let mut state = self.state.get();
        let entry = unsafe { self.entries.get_unchecked(index) };
        entry.prev.set(NONE);
        entry.next.set(state.head);
        if state.head == NONE {
            state.tail = index as u32;
        } else {
            unsafe { self.entries.get_unchecked(state.head as usize) }
                .prev
                .set(index as u32);
        }
        state.head = index as u32;
        state.len += 1;
        self.state.set(state);
    }

    unsafe fn link_back(&self, index: usize) {
        let mut state = self.state.get();
        let entry = unsafe { self.entries.get_unchecked(index) };
        entry.prev.set(state.tail);
        entry.next.set(NONE);
        if state.tail == NONE {
            state.head = index as u32;
        } else {
            unsafe { self.entries.get_unchecked(state.tail as usize) }
                .next
                .set(index as u32);
        }
        state.tail = index as u32;
        state.len += 1;
        self.state.set(state);
    }

    fn pop_front(&self) -> Option<T> {
        self.pop_front_key_value().map(|(_, value)| value)
    }

    fn front(&self) -> Option<&T> {
        let index = (self.state.get().head != NONE).then_some(self.state.get().head as usize)?;
        let value = unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_ref() };
        Some(value)
    }

    fn front_key_value(&self) -> Option<(usize, &T)> {
        let index = (self.state.get().head != NONE).then_some(self.state.get().head as usize)?;
        let value = unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_ref() };
        Some((index, value))
    }

    fn pop_front_key_value(&self) -> Option<(usize, T)> {
        let index = (self.state.get().head != NONE).then_some(self.state.get().head as usize)?;
        Some((index, unsafe { self.remove_unchecked(index) }))
    }

    fn remove(&self, index: usize) -> Option<T> {
        let entry = self.entries.get(index)?;
        if entry.prev.get() == index as u32 {
            return None;
        }
        Some(unsafe { self.remove_unchecked(index) })
    }

    /// # Safety
    /// `index` is in bounds and occupied. No reference returned by `front`
    /// may be live while this method runs.
    unsafe fn remove_unchecked(&self, index: usize) -> T {
        debug_assert!(
            self.entries
                .get(index)
                .is_some_and(|entry| entry.prev.get() != index as u32)
        );
        unsafe { self.unlink(index) };
        unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_read() }
    }

    unsafe fn unlink(&self, index: usize) {
        let mut state = self.state.get();
        let entry = unsafe { self.entries.get_unchecked(index) };
        let prev = entry.prev.get();
        let next = entry.next.get();
        entry.prev.set(index as u32);
        entry.next.set(NONE);
        if prev == NONE {
            state.head = next;
        } else {
            unsafe { self.entries.get_unchecked(prev as usize) }
                .next
                .set(next);
        }
        if next == NONE {
            state.tail = prev;
        } else {
            unsafe { self.entries.get_unchecked(next as usize) }
                .prev
                .set(prev);
        }
        state.len -= 1;
        self.state.set(state);
    }

    fn contains_key(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.prev.get() != index as u32)
    }

    fn clear(&self) {
        while self.pop_front().is_some() {}
    }

    fn grow_to(&mut self, capacity: usize) {
        let old_capacity = self.entries.len();
        assert!(capacity >= old_capacity, "slot queue cannot shrink");
        assert!(
            u32::try_from(capacity).is_ok(),
            "slot queue capacity overflow"
        );
        if capacity == old_capacity {
            return;
        }

        let mut entries = BoxSliceGrowth::take(&mut self.entries);
        entries.reserve_exact(capacity - old_capacity);
        for index in old_capacity..capacity {
            entries.push(Slot::vacant(index as u32));
        }
    }

    fn capacity(&self) -> usize {
        self.entries.len()
    }

    fn len(&self) -> usize {
        self.state.get().len
    }

    fn is_empty(&self) -> bool {
        self.state.get().len == 0
    }
}

pub struct SlotQueue<T = ()> {
    core: SlotQueueCore<T>,
}

pub struct SlotQueueVacantEntry<'a, T> {
    queue: &'a mut SlotQueue<T>,
    index: usize,
}

impl<T> SlotQueueVacantEntry<'_, T> {
    pub fn push_front(self, value: T) {
        unsafe { self.queue.core.push_front_unchecked(self.index, value) };
    }

    pub fn push_back(self, value: T) {
        unsafe { self.queue.core.push_back_unchecked(self.index, value) };
    }
}

impl<T> SlotQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            core: SlotQueueCore::with_capacity(capacity),
        }
    }

    pub fn vacant_entry(&mut self, index: usize) -> Option<SlotQueueVacantEntry<'_, T>> {
        self.core
            .is_vacant(index)
            .then_some(SlotQueueVacantEntry { queue: self, index })
    }

    pub fn push_back(&mut self, index: usize, value: T) -> Result<(), T> {
        self.core.push_back(index, value)
    }

    pub fn push_front(&mut self, index: usize, value: T) -> Result<(), T> {
        self.core.push_front(index, value)
    }

    pub fn refresh_back(&mut self, index: usize, value: T) -> Result<(), T> {
        self.core.refresh_back(index, value).map(drop)
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.core.pop_front()
    }

    pub fn front(&self) -> Option<&T> {
        self.core.front()
    }

    pub fn front_key_value(&self) -> Option<(usize, &T)> {
        self.core.front_key_value()
    }

    pub fn pop_front_key_value(&mut self) -> Option<(usize, T)> {
        self.core.pop_front_key_value()
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        self.core.remove(index)
    }

    pub fn contains_key(&self, index: usize) -> bool {
        self.core.contains_key(index)
    }

    pub fn clear(&mut self) {
        self.core.clear();
    }

    pub fn grow_to(&mut self, capacity: usize) {
        self.core.grow_to(capacity);
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
}

impl<T> Drop for SlotQueue<T> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}

/// A fixed-capacity indexed queue that can be mutated through shared access.
///
/// Values are `Copy`, so queue operations cannot invoke user code while the
/// shared link state is being changed.
pub struct CellSlotQueue<T: Copy = ()> {
    core: SlotQueueCore<T>,
}

impl<T: Copy> CellSlotQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            core: SlotQueueCore::with_capacity(capacity),
        }
    }

    pub fn push_back(&self, index: usize, value: T) -> Result<(), T> {
        self.core.push_back(index, value)
    }

    pub fn push_front(&self, index: usize, value: T) -> Result<(), T> {
        self.core.push_front(index, value)
    }

    pub fn refresh_back(&self, index: usize, value: T) -> Result<(), T> {
        self.core.refresh_back(index, value).map(|_| ())
    }

    pub fn pop_front(&self) -> Option<T> {
        self.core.pop_front()
    }

    pub fn front(&self) -> Option<T> {
        self.core.front().copied()
    }

    pub fn front_key_value(&self) -> Option<(usize, T)> {
        self.core
            .front_key_value()
            .map(|(index, value)| (index, *value))
    }

    pub fn pop_front_key_value(&self) -> Option<(usize, T)> {
        self.core.pop_front_key_value()
    }

    pub fn remove(&self, index: usize) -> Option<T> {
        self.core.remove(index)
    }

    pub fn contains_key(&self, index: usize) -> bool {
        self.core.contains_key(index)
    }

    pub fn clear(&self) {
        self.core.clear();
    }

    pub fn grow_to(&mut self, capacity: usize) {
        self.core.grow_to(capacity);
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
}
