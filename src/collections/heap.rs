use std::{marker, mem, ptr};

use crate::{collections, collections::slab::key};

const NONE: u32 = u32::MAX;

struct Hole<'a, T, F: FnMut(&T, usize)> {
    entries: *mut mem::MaybeUninit<T>,
    len: usize,
    value: mem::ManuallyDrop<T>,
    position: usize,
    on_move: F,
    marker: marker::PhantomData<&'a mut [mem::MaybeUninit<T>]>,
}

impl<'a, T, F: FnMut(&T, usize)> Hole<'a, T, F> {
    unsafe fn with_value(
        entries: &'a mut [mem::MaybeUninit<T>],
        position: usize,
        value: T,
        on_move: F,
    ) -> Self {
        debug_assert!(position < entries.len());
        Self {
            entries: entries.as_mut_ptr(),
            len: entries.len(),
            value: mem::ManuallyDrop::new(value),
            position,
            on_move,
            marker: marker::PhantomData,
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

struct Entry<K> {
    index: usize,
    key: K,
}

pub struct Max<T> {
    entries: Box<[mem::MaybeUninit<T>]>,
    len: usize,
    _thread: crate::ThreadBound,
}

impl<T> Max<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Box::<[T]>::new_uninit_slice(capacity),
            len: 0,
            _thread: Default::default(),
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), T>
    where
        T: Ord,
    {
        if self.len == self.entries.len() {
            return Err(value);
        }
        let position = self.len;
        self.len += 1;
        let mut hole = unsafe {
            Hole::with_value(
                self.entries.get_unchecked_mut(..self.len),
                position,
                value,
                |_, _| {},
            )
        };
        hole.sift_up(0, &mut |left, right| left > right);
        Ok(())
    }

    pub fn peek(&self) -> Option<&T> {
        (self.len != 0).then(|| unsafe { self.entries.get_unchecked(0).assume_init_ref() })
    }

    pub fn pop_if(&mut self, predicate: impl FnOnce(&T) -> bool) -> Option<T>
    where
        T: Ord,
    {
        let first = self.peek()?;
        if predicate(first) { self.pop() } else { None }
    }

    pub fn pop(&mut self) -> Option<T>
    where
        T: Ord,
    {
        if self.is_empty() {
            return None;
        }
        let value = unsafe { self.entries.get_unchecked(0).assume_init_read() };
        self.len -= 1;
        if self.len != 0 {
            let tail = unsafe { self.entries.get_unchecked(self.len).assume_init_read() };
            let mut hole = unsafe {
                Hole::with_value(
                    self.entries.get_unchecked_mut(..self.len),
                    0,
                    tail,
                    |_, _| {},
                )
            };
            hole.sift_down(&mut |left, right| left > right);
        }
        Some(value)
    }

    pub fn clear(&mut self) {
        while !self.is_empty() {
            self.len -= 1;
            unsafe { self.entries.get_unchecked_mut(self.len).assume_init_drop() };
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T> Drop for Max<T> {
    fn drop(&mut self) {
        collections::ClearGuard::run(self, Self::clear);
    }
}

pub struct Min<K: Ord> {
    entries: Box<[mem::MaybeUninit<Entry<K>>]>,
    positions: Box<[u32]>,
    len: usize,
    _thread: crate::ThreadBound,
}

pub struct Vacant<'a, K: Ord> {
    heap: &'a mut Min<K>,
    index: usize,
}

impl<K: Ord> Vacant<'_, K> {
    pub fn insert(self, key: K) {
        unsafe { self.heap.insert_unchecked(self.index, key) };
    }
}

impl<K: Ord> Min<K> {
    pub fn new() -> Self {
        Self {
            entries: Box::default(),
            positions: Box::default(),
            len: 0,
            _thread: Default::default(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(heap) => heap,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        assert!(
            u32::try_from(capacity).is_ok(),
            "index heap capacity overflow"
        );
        Ok(Self {
            entries: collections::try_box_uninit(capacity)?,
            positions: collections::try_box_with(capacity, |_| NONE)?,
            len: 0,
            _thread: Default::default(),
        })
    }

    pub fn vacant_entry(&mut self, index: usize) -> Option<Vacant<'_, K>> {
        self.positions
            .get(index)
            .is_some_and(|position| *position == NONE)
            .then_some(Vacant { heap: self, index })
    }

    pub fn insert(&mut self, index: usize, key: K) -> Result<(), K> {
        if self
            .positions
            .get(index)
            .is_none_or(|position| *position != NONE)
        {
            return Err(key);
        }
        unsafe { self.insert_unchecked(index, key) };
        Ok(())
    }

    /// Inserts or replaces one indexed key, growing to the proven slot when needed.
    pub fn set<Tag, const MAX: u32>(&mut self, slot: key::Key<Tag, MAX>, key: K) {
        let index = slot.index() as usize;
        if index >= self.capacity() {
            self.grow_to(index + 1);
        }
        let position = self.positions[index];
        if position == NONE {
            unsafe { self.insert_unchecked(index, key) };
            return;
        }

        let position = position as usize;
        let previous = unsafe { self.entries.get_unchecked(position).assume_init_read() };
        debug_assert_eq!(previous.index, index);
        let value = Entry { index, key };
        let positions = &mut self.positions;
        let on_move = |entry: &Entry<K>, position: usize| unsafe {
            *positions.get_unchecked_mut(entry.index) = position as u32;
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
        drop(previous.key);
    }

    /// # Safety
    /// `index < capacity()`, its slot is vacant, and the heap is not full.
    unsafe fn insert_unchecked(&mut self, index: usize, key: K) {
        debug_assert!(
            self.positions
                .get(index)
                .is_some_and(|position| *position == NONE)
        );
        debug_assert!(self.len < self.entries.len());
        let position = self.len;
        self.len += 1;
        let value = Entry { index, key };
        let positions = &mut self.positions;
        let on_move = |entry: &Entry<K>, position: usize| unsafe {
            *positions.get_unchecked_mut(entry.index) = position as u32;
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

    pub fn peek(&self) -> Option<(usize, &K)> {
        (self.len != 0).then(|| {
            let entry = self.entry(0);
            (entry.index, &entry.key)
        })
    }

    pub fn pop(&mut self) -> Option<(usize, K)> {
        (self.len != 0).then(|| unsafe { self.remove_position(0) })
    }

    /// Removes the minimum only when it satisfies `predicate`.
    pub fn pop_if(&mut self, predicate: impl FnOnce(&K) -> bool) -> Option<(usize, K)> {
        let (_, key) = self.peek()?;
        if predicate(key) { self.pop() } else { None }
    }

    pub fn remove(&mut self, index: usize) -> Option<K> {
        let position = *self.positions.get(index)?;
        if position == NONE || self.entry(position as usize).index != index {
            return None;
        }
        Some(unsafe { self.remove_position(position as usize).1 })
    }

    unsafe fn remove_position(&mut self, position: usize) -> (usize, K) {
        let entry = unsafe { self.entries.get_unchecked(position).assume_init_read() };
        self.len -= 1;
        unsafe { *self.positions.get_unchecked_mut(entry.index) = NONE };
        if position < self.len {
            let value = unsafe { self.entries.get_unchecked(self.len).assume_init_read() };
            let positions = &mut self.positions;
            let on_move = |entry: &Entry<K>, position: usize| unsafe {
                *positions.get_unchecked_mut(entry.index) = position as u32;
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
        use crate::collections::BoxSliceGrowth;
        let old_capacity = self.positions.len();
        assert!(capacity >= old_capacity, "index heap cannot shrink");
        assert!(
            u32::try_from(capacity).is_ok(),
            "index heap capacity overflow"
        );
        if capacity == old_capacity {
            return;
        }

        let additional = capacity - old_capacity;
        let mut entries = BoxSliceGrowth::take(&mut self.entries);
        let mut positions = BoxSliceGrowth::take(&mut self.positions);
        entries.reserve_exact(additional);
        positions.reserve_exact(additional);
        entries.resize_with(capacity, mem::MaybeUninit::uninit);
        positions.resize(capacity, NONE);
    }

    pub fn clear(&mut self) {
        while self.len > 0 {
            let position = self.len - 1;
            let index = self.entry(position).index;
            self.positions[index] = NONE;
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

    fn entry(&self, position: usize) -> &Entry<K> {
        debug_assert!(position < self.len);
        unsafe { self.entries.get_unchecked(position).assume_init_ref() }
    }
}

impl<K: Ord> Default for Min<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord> Drop for Min<K> {
    fn drop(&mut self) {
        collections::ClearGuard::run(self, Self::clear);
    }
}
