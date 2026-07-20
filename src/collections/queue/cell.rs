use std::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;

use crate::collections::ClearGuard;
use crate::marker::ThreadBound;

pub struct CellQueue<T> {
    entries: Box<[UnsafeCell<MaybeUninit<T>>]>,
    capacity: usize,
    head: Cell<usize>,
    tail: Cell<usize>,
    _thread: ThreadBound,
}

impl<T> CellQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            capacity.checked_next_power_of_two().is_some(),
            "cell queue capacity overflow"
        );
        let ring = capacity.next_power_of_two();
        Self {
            entries: (0..ring)
                .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
                .collect(),
            capacity,
            head: Cell::new(0),
            tail: Cell::new(0),
            _thread: ThreadBound::NEW,
        }
    }

    fn mask(&self) -> usize {
        self.entries.len() - 1
    }

    pub fn push_back(&self, value: T) -> Result<(), T> {
        let tail = self.tail.get();
        if tail.wrapping_sub(self.head.get()) == self.capacity {
            return Err(value);
        }
        unsafe { self.push_back_unchecked(value) };
        Ok(())
    }

    /// # Safety
    /// `len() < capacity()`.
    unsafe fn push_back_unchecked(&self, value: T) {
        let tail = self.tail.get();
        unsafe {
            (*self.entries.get_unchecked(tail & self.mask()).get()).write(value);
        }
        self.tail.set(tail.wrapping_add(1));
    }

    pub fn pop_front(&self) -> Option<T> {
        let head = self.head.get();
        if head == self.tail.get() {
            return None;
        }
        let value =
            unsafe { (*self.entries.get_unchecked(head & self.mask()).get()).assume_init_read() };
        self.head.set(head.wrapping_add(1));
        Some(value)
    }

    pub fn clear(&self) {
        while let Some(value) = self.pop_front() {
            drop(value);
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.tail.get().wrapping_sub(self.head.get())
    }

    pub fn is_empty(&self) -> bool {
        self.head.get() == self.tail.get()
    }

    pub fn is_full(&self) -> bool {
        self.len() == self.capacity
    }
}

impl<T> Drop for CellQueue<T> {
    fn drop(&mut self) {
        ClearGuard::run(self, |queue| queue.clear());
    }
}
