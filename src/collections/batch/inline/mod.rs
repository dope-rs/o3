mod sealed;

pub(crate) use sealed::Slots;

/// Fixed-capacity storage filled while empty and drained in FIFO order.
pub struct Inline<T, const N: usize> {
    slots: Slots<T, N>,
    head: usize,
    tail: usize,
}

/// Exclusive proof that an [`Inline`] batch may be filled.
#[must_use = "an empty batch proof does nothing unless it is used to fill the batch"]
#[repr(transparent)]
pub struct Fill<'a, T, const N: usize> {
    batch: &'a mut Inline<T, N>,
}

/// A vacant position at the back of a batch being filled.
#[repr(transparent)]
pub struct Vacant<'a, T, const N: usize> {
    batch: &'a mut Inline<T, N>,
}

/// A reserved position vacated by a front pop.
#[repr(transparent)]
pub struct FrontVacant<'a, T, const N: usize> {
    batch: &'a mut Inline<T, N>,
}

impl<T, const N: usize> Inline<T, N> {
    pub const fn new() -> Self {
        Self {
            slots: Slots::new(),
            head: 0,
            tail: 0,
        }
    }

    /// Borrows this batch for production only while it is empty.
    pub fn fill(&mut self) -> Option<Fill<'_, T, N>> {
        if !self.is_empty() {
            return None;
        }
        self.head = 0;
        self.tail = 0;
        Some(Fill { batch: self })
    }

    pub fn pop_front(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let index = self.head;
        self.head += 1;
        Some(self.slots.take(index))
    }

    /// Pops the front while reserving its position for rollback.
    pub fn pop_front_reserved(&mut self) -> Option<(T, FrontVacant<'_, T, N>)> {
        let value = self.pop_front()?;
        Some((value, FrontVacant { batch: self }))
    }

    pub fn clear(&mut self) {
        while let Some(value) = self.pop_front() {
            drop(value);
        }
        self.head = 0;
        self.tail = 0;
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub const fn len(&self) -> usize {
        self.tail - self.head
    }

    pub const fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

impl<T, const N: usize> Fill<'_, T, N> {
    pub fn vacant_entry(&mut self) -> Option<Vacant<'_, T, N>> {
        if self.batch.tail == N {
            None
        } else {
            Some(Vacant { batch: self.batch })
        }
    }
}

impl<T, const N: usize> Vacant<'_, T, N> {
    pub fn insert(self, value: T) {
        let index = self.batch.tail;
        self.batch.slots.write(index, value);
        self.batch.tail = index + 1;
    }
}

impl<T, const N: usize> FrontVacant<'_, T, N> {
    pub fn restore(self, value: T) {
        self.batch.head -= 1;
        self.batch.slots.write(self.batch.head, value);
    }
}

impl<T, const N: usize> Default for Inline<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for Inline<T, N> {
    fn drop(&mut self) {
        self.clear();
    }
}
