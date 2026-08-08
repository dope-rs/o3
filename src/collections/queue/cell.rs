use std::{cell, mem};

pub struct Fifo<T> {
    entries: Box<[cell::UnsafeCell<mem::MaybeUninit<T>>]>,
    capacity: usize,
    head: cell::Cell<usize>,
    tail: cell::Cell<usize>,
    _thread: crate::ThreadBound,
}

impl<T> Fifo<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        use crate::ThreadBound;
        assert!(
            capacity.checked_next_power_of_two().is_some(),
            "cell queue capacity overflow"
        );
        let ring = capacity.next_power_of_two();
        Self {
            entries: (0..ring)
                .map(|_| cell::UnsafeCell::new(mem::MaybeUninit::uninit()))
                .collect(),
            capacity,
            head: cell::Cell::new(0),
            tail: cell::Cell::new(0),
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

    pub fn len(&self) -> usize {
        self.tail.get().wrapping_sub(self.head.get())
    }

    pub fn is_empty(&self) -> bool {
        self.head.get() == self.tail.get()
    }
}

impl<T> Drop for Fifo<T> {
    fn drop(&mut self) {
        use crate::collections::ClearGuard;
        ClearGuard::run(self, |queue| queue.clear());
    }
}
