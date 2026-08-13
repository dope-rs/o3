use std::mem;

use crate::collections::{self, queue::fixed};

pub struct Fifo<T> {
    ring: Ring<T>,
}

pub struct Coalescing<T, I = u32> {
    pending: Fifo<I>,
    entries: Box<[Option<T>]>,
}

struct Ring<T> {
    entries: Box<[mem::MaybeUninit<T>]>,
    head: usize,
    len: usize,
    _thread: crate::ThreadBound,
}

pub struct Vacant<'a, T> {
    queue: &'a mut Fifo<T>,
}

impl<T> Vacant<'_, T> {
    pub fn push_back(self, value: T) {
        unsafe { self.queue.ring.push_back_unchecked(value) };
    }

    pub fn push_front(self, value: T) {
        unsafe { self.queue.ring.push_front_unchecked(value) };
    }
}

impl<T> Coalescing<T> {
    pub fn with_capacity(capacity: u32) -> Self {
        Self::with_index_capacity(capacity)
    }

    pub fn try_with_capacity(capacity: u32) -> Result<Self, collections::AllocationError> {
        Self::try_with_index_capacity(capacity)
    }
}

impl<T, I> Coalescing<T, I>
where
    I: fixed::raw::Index,
{
    pub fn with_index_capacity(capacity: u32) -> Self {
        match Self::try_with_index_capacity(capacity) {
            Ok(queue) => queue,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_index_capacity(capacity: u32) -> Result<Self, collections::AllocationError> {
        let capacity = capacity as usize;
        Ok(Self {
            pending: Fifo::try_with_capacity(capacity)?,
            entries: collections::BoxSliceExt::try_box_with(capacity, |_| None)?,
        })
    }

    pub fn schedule(&mut self, index: I, value: T) -> Result<(), T> {
        if index.index() as usize >= self.entries.len() {
            return Err(value);
        }
        unsafe { fixed::raw::Coalescing::schedule_unchecked(self, index, value) };
        Ok(())
    }

    pub fn pop_front(&mut self) -> Option<(I, T)> {
        let index = self.pending.pop_front()?;
        let entry = unsafe { self.entries.get_unchecked_mut(index.index() as usize) };
        let value = unsafe { entry.take().unwrap_unchecked() };
        Some((index, value))
    }

    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl<T> Fifo<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(queue) => queue,
            Err(error) => error.abort(),
        }
    }

    /// Reserves the complete fixed backing ring without invoking the global
    /// allocation-error handler.
    pub fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        use crate::ThreadBound;
        Ok(Self {
            ring: Ring {
                entries: collections::BoxUninitExt::try_box_uninit(capacity)?,
                head: 0,
                len: 0,
                _thread: ThreadBound::NEW,
            },
        })
    }

    pub fn vacant_entry(&mut self) -> Option<Vacant<'_, T>> {
        (self.ring.len != self.ring.entries.len()).then_some(Vacant { queue: self })
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        unsafe { self.ring.push_back_unchecked(value) };
        Ok(())
    }

    pub fn push_front(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        unsafe { self.ring.push_front_unchecked(value) };
        Ok(())
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.ring.pop_front()
    }

    pub fn pop_front_reserved(&mut self) -> Option<(T, Vacant<'_, T>)> {
        let value = self.ring.pop_front()?;
        Some((value, Vacant { queue: self }))
    }

    pub fn front(&self) -> Option<&T> {
        (self.ring.len != 0).then(|| unsafe {
            self.ring
                .entries
                .get_unchecked(self.ring.head)
                .assume_init_ref()
        })
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        (index < self.ring.len).then(|| {
            let position = self.ring.position(index);
            unsafe { self.ring.entries.get_unchecked(position).assume_init_ref() }
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.ring.len).map(move |index| unsafe {
            let position = self.ring.position(index);
            self.ring.entries.get_unchecked(position).assume_init_ref()
        })
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let len = self.ring.len;
        for _ in 0..len {
            let value = unsafe { self.ring.pop_front_unchecked() };
            if keep(&value) {
                unsafe { self.ring.push_back_unchecked(value) };
            }
        }
    }

    pub fn clear(&mut self) {
        while let Some(value) = self.pop_front() {
            drop(value);
        }
    }

    pub fn capacity(&self) -> usize {
        self.ring.entries.len()
    }

    pub fn len(&self) -> usize {
        self.ring.len
    }

    pub fn is_empty(&self) -> bool {
        self.ring.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.ring.len == self.ring.entries.len()
    }
}

impl<T> Ring<T> {
    fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        Some(unsafe { self.pop_front_unchecked() })
    }

    /// # Safety
    /// `len < capacity`.
    unsafe fn push_back_unchecked(&mut self, value: T) {
        let index = self.position(self.len);
        unsafe { self.entries.get_unchecked_mut(index).write(value) };
        self.len += 1;
    }

    /// # Safety
    /// `len < capacity`.
    unsafe fn push_front_unchecked(&mut self, value: T) {
        self.head = if self.head == 0 {
            self.entries.len() - 1
        } else {
            self.head - 1
        };
        unsafe { self.entries.get_unchecked_mut(self.head).write(value) };
        self.len += 1;
    }

    /// # Safety
    /// The ring is not empty.
    unsafe fn pop_front_unchecked(&mut self) -> T {
        let value = unsafe { self.entries.get_unchecked(self.head).assume_init_read() };
        self.head += 1;
        if self.head == self.entries.len() {
            self.head = 0;
        }
        self.len -= 1;
        value
    }

    fn position(&self, index: usize) -> usize {
        let tail = self.entries.len() - self.head;
        if index < tail {
            self.head + index
        } else {
            index - tail
        }
    }
}

impl<T> Drop for Fifo<T> {
    fn drop(&mut self) {
        use crate::collections::ClearGuard;
        ClearGuard::run(self, Self::clear);
    }
}

impl<T, I> fixed::raw::Coalescing<T, I> for Coalescing<T, I>
where
    I: fixed::raw::Index,
{
    unsafe fn schedule_unchecked(&mut self, index: I, value: T) {
        let entry = unsafe { self.entries.get_unchecked_mut(index.index() as usize) };
        if entry.is_none() {
            unsafe { self.pending.ring.push_back_unchecked(index) };
        }
        *entry = Some(value);
    }
}

impl<T> fixed::raw::Fifo<T> for Fifo<T> {
    unsafe fn vacant_entry_unchecked<'queue>(&'queue mut self) -> Vacant<'queue, T> {
        Vacant { queue: self }
    }
}
