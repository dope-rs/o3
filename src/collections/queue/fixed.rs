use std::mem;

pub struct Fifo<T> {
    ring: Ring<T>,
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

impl<T> Fifo<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        use crate::ThreadBound;
        Self {
            ring: Ring {
                entries: Box::<[T]>::new_uninit_slice(capacity),
                head: 0,
                len: 0,
                _thread: ThreadBound::NEW,
            },
        }
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
        if self.ring.len == 0 {
            return None;
        }
        Some(unsafe { self.ring.pop_front_unchecked() })
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

    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.iter().any(|entry| entry == value)
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
