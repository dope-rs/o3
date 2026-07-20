use std::marker::PhantomData;
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ptr;

use crate::collections::{ClearGuard, IndexKey, Storage};
use crate::marker::ThreadBound;

const NONE: u32 = u32::MAX;

struct Hole<'a, T, F: FnMut(&T, usize)> {
    entries: *mut MaybeUninit<T>,
    len: usize,
    value: ManuallyDrop<T>,
    position: usize,
    on_move: F,
    marker: PhantomData<&'a mut [MaybeUninit<T>]>,
}

impl<'a, T, F: FnMut(&T, usize)> Hole<'a, T, F> {
    unsafe fn with_value(
        entries: &'a mut [MaybeUninit<T>],
        position: usize,
        value: T,
        on_move: F,
    ) -> Self {
        debug_assert!(position < entries.len());
        Self {
            entries: entries.as_mut_ptr(),
            len: entries.len(),
            value: ManuallyDrop::new(value),
            position,
            on_move,
            marker: PhantomData,
        }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn element(&self) -> &T {
        &self.value
    }

    unsafe fn get(&self, position: usize) -> &T {
        debug_assert!(position < self.len && position != self.position);
        unsafe { (&*self.entries.add(position)).assume_init_ref() }
    }

    unsafe fn move_to(&mut self, position: usize) {
        debug_assert!(position < self.len && position != self.position);
        let source = unsafe { (*self.entries.add(position)).as_ptr() };
        (self.on_move)(unsafe { &*source }, self.position);
        unsafe {
            ptr::copy_nonoverlapping(source, (*self.entries.add(self.position)).as_mut_ptr(), 1);
        }
        self.position = position;
    }
}

impl<T, F: FnMut(&T, usize)> Drop for Hole<'_, T, F> {
    fn drop(&mut self) {
        (self.on_move)(&self.value, self.position);
        unsafe {
            ptr::copy_nonoverlapping(
                &*self.value,
                (*self.entries.add(self.position)).as_mut_ptr(),
                1,
            );
        }
    }
}

impl<T, F: FnMut(&T, usize)> Hole<'_, T, F> {
    fn sift_up<P: FnMut(&T, &T) -> bool>(&mut self, start: usize, precedes: &mut P) {
        while self.position() > start {
            let parent = (self.position() - 1) / 2;
            if !precedes(self.element(), unsafe { self.get(parent) }) {
                return;
            }
            unsafe { self.move_to(parent) };
        }
    }

    fn sift_down<P: FnMut(&T, &T) -> bool>(&mut self, precedes: &mut P) {
        if self.len < 2 {
            return;
        }
        while self.position() <= (self.len - 2) / 2 {
            let left = self.position() * 2 + 1;
            let right = left + 1;
            let child = if right < self.len
                && precedes(unsafe { self.get(right) }, unsafe { self.get(left) })
            {
                right
            } else {
                left
            };
            if !precedes(unsafe { self.get(child) }, self.element()) {
                return;
            }
            unsafe { self.move_to(child) };
        }
    }

    fn repair<P: FnMut(&T, &T) -> bool>(&mut self, precedes: &mut P) {
        let position = self.position();
        if position != 0 {
            let parent = (position - 1) / 2;
            if precedes(self.element(), unsafe { self.get(parent) }) {
                self.sift_up(0, precedes);
                return;
            }
        }
        self.sift_down(precedes);
    }
}

struct HeapEntry<I, K> {
    index: I,
    key: K,
}

pub struct IndexedMinHeap<K: Ord, I: IndexKey = usize> {
    entries: Box<[MaybeUninit<HeapEntry<I, K>>]>,
    positions: Box<[u32]>,
    len: usize,
    _thread: ThreadBound,
}

pub struct IndexedMinHeapVacantEntry<'a, K: Ord, I: IndexKey = usize> {
    heap: &'a mut IndexedMinHeap<K, I>,
    index: I,
}

impl<K: Ord, I: IndexKey> IndexedMinHeapVacantEntry<'_, K, I> {
    pub fn insert(self, key: K) {
        unsafe { self.heap.insert_unchecked(self.index, key) };
    }
}

impl<K: Ord, I: IndexKey> IndexedMinHeap<K, I> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "index heap capacity overflow"
        );
        Self {
            entries: Storage::uninit_boxed_slice(capacity),
            positions: vec![NONE; capacity].into_boxed_slice(),
            len: 0,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn vacant_entry(&mut self, index: I) -> Option<IndexedMinHeapVacantEntry<'_, K, I>> {
        let raw = index.index();
        self.positions
            .get(raw)
            .is_some_and(|position| *position == NONE)
            .then_some(IndexedMinHeapVacantEntry { heap: self, index })
    }

    pub fn insert(&mut self, index: I, key: K) -> Result<(), K> {
        let raw = index.index();
        if self
            .positions
            .get(raw)
            .is_none_or(|position| *position != NONE)
        {
            return Err(key);
        }
        unsafe { self.insert_unchecked(index, key) };
        Ok(())
    }

    /// # Safety
    /// `index.index() < capacity()`, its slot is vacant, and the heap is not full.
    unsafe fn insert_unchecked(&mut self, index: I, key: K) {
        let raw = index.index();
        debug_assert!(
            self.positions
                .get(raw)
                .is_some_and(|position| *position == NONE)
        );
        debug_assert!(self.len < self.entries.len());
        let position = self.len;
        self.len += 1;
        let value = HeapEntry { index, key };
        let positions = &mut self.positions;
        let on_move = |entry: &HeapEntry<I, K>, position: usize| unsafe {
            *positions.get_unchecked_mut(entry.index.index()) = position as u32;
        };
        let mut hole = unsafe {
            Hole::with_value(
                self.entries.get_unchecked_mut(..self.len),
                position,
                value,
                on_move,
            )
        };
        hole.sift_up(0, &mut |left, right| left.key < right.key);
    }

    pub fn peek(&self) -> Option<(I, &K)> {
        (self.len != 0).then(|| {
            let entry = self.entry(0);
            (entry.index, &entry.key)
        })
    }

    pub fn pop(&mut self) -> Option<(I, K)> {
        (self.len != 0).then(|| unsafe { self.remove_position(0) })
    }

    pub fn remove(&mut self, index: I) -> Option<K> {
        let position = *self.positions.get(index.index())?;
        if position == NONE || self.entry(position as usize).index != index {
            return None;
        }
        Some(unsafe { self.remove_position(position as usize).1 })
    }

    unsafe fn remove_position(&mut self, position: usize) -> (I, K) {
        let entry = unsafe { self.entries.get_unchecked(position).assume_init_read() };
        self.len -= 1;
        unsafe { *self.positions.get_unchecked_mut(entry.index.index()) = NONE };
        if position < self.len {
            let value = unsafe { self.entries.get_unchecked(self.len).assume_init_read() };
            let positions = &mut self.positions;
            let on_move = |entry: &HeapEntry<I, K>, position: usize| unsafe {
                *positions.get_unchecked_mut(entry.index.index()) = position as u32;
            };
            let mut hole = unsafe {
                Hole::with_value(
                    self.entries.get_unchecked_mut(..self.len),
                    position,
                    value,
                    on_move,
                )
            };
            hole.repair(&mut |left, right| left.key < right.key);
        }
        (entry.index, entry.key)
    }

    pub fn grow_to(&mut self, capacity: usize) {
        assert!(capacity >= self.positions.len(), "index heap cannot shrink");
        assert!(
            u32::try_from(capacity).is_ok(),
            "index heap capacity overflow"
        );
        if capacity == self.positions.len() {
            return;
        }
        let mut entries = Storage::uninit_boxed_slice(capacity);
        for position in 0..self.len {
            entries[position] = mem::replace(&mut self.entries[position], MaybeUninit::uninit());
        }
        let mut positions = vec![NONE; capacity].into_boxed_slice();
        positions[..self.positions.len()].copy_from_slice(&self.positions);
        self.entries = entries;
        self.positions = positions;
    }

    pub fn contains_key(&self, index: I) -> bool {
        let Some(&position) = self.positions.get(index.index()) else {
            return false;
        };
        position != NONE && self.entry(position as usize).index == index
    }

    pub fn clear(&mut self) {
        while self.len > 0 {
            let position = self.len - 1;
            let index = self.entry(position).index;
            self.positions[index.index()] = NONE;
            self.len -= 1;
            unsafe { self.entries.get_unchecked_mut(position).assume_init_drop() };
        }
    }

    pub fn capacity(&self) -> usize {
        self.positions.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn entry(&self, position: usize) -> &HeapEntry<I, K> {
        debug_assert!(position < self.len);
        unsafe { self.entries.get_unchecked(position).assume_init_ref() }
    }
}

impl<K: Ord, I: IndexKey> Drop for IndexedMinHeap<K, I> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}
