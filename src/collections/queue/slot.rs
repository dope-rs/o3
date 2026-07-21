use std::mem::MaybeUninit;

use crate::collections::ClearGuard;
use crate::collections::grow::BoxSliceGrowth;
use crate::marker::ThreadBound;

const NONE: u32 = u32::MAX;

struct Slot<T> {
    value: MaybeUninit<T>,
    prev: u32,
    next: u32,
}

impl<T> Slot<T> {
    fn vacant(index: u32) -> Self {
        Self {
            value: MaybeUninit::uninit(),
            prev: index,
            next: NONE,
        }
    }
}

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

pub struct SlotQueue<T = ()> {
    entries: Box<[Slot<T>]>,
    state: State,
    _thread: ThreadBound,
}

pub struct SlotQueueVacantEntry<'a, T> {
    queue: &'a mut SlotQueue<T>,
    index: usize,
}

impl<T> SlotQueueVacantEntry<'_, T> {
    pub fn push_front(self, value: T) {
        unsafe { self.queue.push_front_unchecked(self.index, value) };
    }

    pub fn push_back(self, value: T) {
        unsafe { self.queue.push_back_unchecked(self.index, value) };
    }
}

impl<T> SlotQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "slot queue capacity overflow"
        );
        Self {
            entries: (0..capacity as u32).map(Slot::vacant).collect(),
            state: State::EMPTY,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn vacant_entry(&mut self, index: usize) -> Option<SlotQueueVacantEntry<'_, T>> {
        self.is_vacant(index)
            .then_some(SlotQueueVacantEntry { queue: self, index })
    }

    pub fn push_back(&mut self, index: usize, value: T) -> Result<(), T> {
        if !self.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.push_back_unchecked(index, value) };
        Ok(())
    }

    pub fn push_front(&mut self, index: usize, value: T) -> Result<(), T> {
        if !self.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.push_front_unchecked(index, value) };
        Ok(())
    }

    /// # Safety
    /// `index` is in bounds and vacant.
    unsafe fn push_front_unchecked(&mut self, index: usize, value: T) {
        debug_assert!(self.is_vacant(index));
        unsafe { self.entries.get_unchecked_mut(index).value.write(value) };
        unsafe { self.link_front(index) };
    }

    /// # Safety
    /// `index` is in bounds and vacant.
    unsafe fn push_back_unchecked(&mut self, index: usize, value: T) {
        debug_assert!(self.is_vacant(index));
        unsafe { self.entries.get_unchecked_mut(index).value.write(value) };
        unsafe { self.link_back(index) };
    }

    fn is_vacant(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.prev == index as u32)
    }

    unsafe fn link_front(&mut self, index: usize) {
        let entry = unsafe { self.entries.get_unchecked_mut(index) };
        entry.prev = NONE;
        entry.next = self.state.head;
        if self.state.head == NONE {
            self.state.tail = index as u32;
        } else {
            unsafe { self.entries.get_unchecked_mut(self.state.head as usize) }.prev = index as u32;
        }
        self.state.head = index as u32;
        self.state.len += 1;
    }

    unsafe fn link_back(&mut self, index: usize) {
        let entry = unsafe { self.entries.get_unchecked_mut(index) };
        entry.prev = self.state.tail;
        entry.next = NONE;
        if self.state.tail == NONE {
            self.state.head = index as u32;
        } else {
            unsafe { self.entries.get_unchecked_mut(self.state.tail as usize) }.next = index as u32;
        }
        self.state.tail = index as u32;
        self.state.len += 1;
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.pop_front_key_value().map(|(_, value)| value)
    }

    pub fn front(&self) -> Option<&T> {
        let index = (self.state.head != NONE).then_some(self.state.head as usize)?;
        Some(unsafe { self.entries.get_unchecked(index).value.assume_init_ref() })
    }

    pub fn front_key_value(&self) -> Option<(usize, &T)> {
        let index = (self.state.head != NONE).then_some(self.state.head as usize)?;
        let value = unsafe { self.entries.get_unchecked(index).value.assume_init_ref() };
        Some((index, value))
    }

    pub fn pop_front_key_value(&mut self) -> Option<(usize, T)> {
        let index = (self.state.head != NONE).then_some(self.state.head as usize)?;
        Some((index, unsafe { self.remove_unchecked(index) }))
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        let entry = self.entries.get(index)?;
        if entry.prev == index as u32 {
            return None;
        }
        Some(unsafe { self.remove_unchecked(index) })
    }

    /// # Safety
    /// `index` is in bounds and occupied.
    unsafe fn remove_unchecked(&mut self, index: usize) -> T {
        debug_assert!(
            self.entries
                .get(index)
                .is_some_and(|entry| entry.prev != index as u32)
        );
        unsafe { self.unlink(index) };
        unsafe {
            self.entries
                .get_unchecked_mut(index)
                .value
                .assume_init_read()
        }
    }

    unsafe fn unlink(&mut self, index: usize) {
        let entry = unsafe { self.entries.get_unchecked_mut(index) };
        let prev = entry.prev;
        let next = entry.next;
        entry.prev = index as u32;
        entry.next = NONE;
        if prev == NONE {
            self.state.head = next;
        } else {
            unsafe { self.entries.get_unchecked_mut(prev as usize) }.next = next;
        }
        if next == NONE {
            self.state.tail = prev;
        } else {
            unsafe { self.entries.get_unchecked_mut(next as usize) }.prev = prev;
        }
        self.state.len -= 1;
    }

    pub fn contains_key(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.prev != index as u32)
    }

    pub fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }

    pub fn grow_to(&mut self, capacity: usize) {
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

    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.state.len
    }

    pub fn is_empty(&self) -> bool {
        self.state.len == 0
    }
}

impl<T> Drop for SlotQueue<T> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}
