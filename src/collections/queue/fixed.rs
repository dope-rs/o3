use std::mem::MaybeUninit;

use crate::collections::ClearGuard;
use crate::marker::ThreadBound;

pub struct FixedQueue<T> {
    entries: Box<[MaybeUninit<T>]>,
    head: usize,
    len: usize,
    _thread: ThreadBound,
}

pub struct FixedQueueVacantEntry<'a, T> {
    queue: &'a mut FixedQueue<T>,
}

impl<T> FixedQueueVacantEntry<'_, T> {
    pub fn push_back(self, value: T) {
        unsafe { self.queue.push_back_unchecked(value) };
    }

    pub fn push_front(self, value: T) {
        unsafe { self.queue.push_front_unchecked(value) };
    }
}

impl<T> FixedQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Box::<[T]>::new_uninit_slice(capacity),
            head: 0,
            len: 0,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn vacant_entry(&mut self) -> Option<FixedQueueVacantEntry<'_, T>> {
        (self.len != self.entries.len()).then_some(FixedQueueVacantEntry { queue: self })
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        unsafe { self.push_back_unchecked(value) };
        Ok(())
    }

    /// # Safety
    /// `len() < capacity()`.
    unsafe fn push_back_unchecked(&mut self, value: T) {
        let index = self.position(self.len);
        unsafe { self.entries.get_unchecked_mut(index).write(value) };
        self.len += 1;
    }

    pub fn push_front(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        unsafe { self.push_front_unchecked(value) };
        Ok(())
    }

    /// # Safety
    /// `len() < capacity()`.
    unsafe fn push_front_unchecked(&mut self, value: T) {
        self.head = if self.head == 0 {
            self.entries.len() - 1
        } else {
            self.head - 1
        };
        unsafe { self.entries.get_unchecked_mut(self.head).write(value) };
        self.len += 1;
    }

    pub fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        Some(unsafe { self.pop_front_unchecked() })
    }

    unsafe fn pop_front_unchecked(&mut self) -> T {
        let value = unsafe { self.entries.get_unchecked(self.head).assume_init_read() };
        self.head += 1;
        if self.head == self.entries.len() {
            self.head = 0;
        }
        self.len -= 1;
        value
    }

    pub fn front(&self) -> Option<&T> {
        (self.len != 0).then(|| unsafe { self.entries.get_unchecked(self.head).assume_init_ref() })
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        (index < self.len).then(|| {
            let position = self.position(index);
            unsafe { self.entries.get_unchecked(position).assume_init_ref() }
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.len).map(move |index| unsafe {
            let position = self.position(index);
            self.entries.get_unchecked(position).assume_init_ref()
        })
    }

    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.iter().any(|entry| entry == value)
    }

    fn position(&self, index: usize) -> usize {
        let tail = self.entries.len() - self.head;
        if index < tail {
            self.head + index
        } else {
            index - tail
        }
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let len = self.len;
        for _ in 0..len {
            let value = unsafe { self.pop_front_unchecked() };
            if keep(&value) {
                unsafe { self.push_back_unchecked(value) };
            }
        }
    }

    pub fn clear(&mut self) {
        while let Some(value) = self.pop_front() {
            drop(value);
        }
    }

    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len == self.entries.len()
    }
}

impl<T> Drop for FixedQueue<T> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}
