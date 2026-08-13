use std::mem;

/// A fixed-capacity inline batch filled only while empty and drained in FIFO order.
///
/// Unlike a ring queue, consumed capacity is not reused until the batch becomes
/// empty and a new [`Fill`] is acquired. This keeps fill and drain index math
/// linear and makes a partially drained batch immutable to producers.
pub struct Inline<T, const N: usize> {
    slots: [mem::MaybeUninit<T>; N],
    head: usize,
    tail: usize,
}

/// Exclusive proof that an [`Inline`] batch is empty and may be filled.
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

/// The position vacated by a reserved front pop.
///
/// Retaining the exclusive borrow prevents the queue from changing before a
/// failed operation restores its value to the original front position.
#[repr(transparent)]
pub struct FrontVacant<'a, T, const N: usize> {
    batch: &'a mut Inline<T, N>,
}

impl<T, const N: usize> Inline<T, N> {
    #[inline]
    pub const fn new() -> Self {
        Self {
            slots: [const { mem::MaybeUninit::uninit() }; N],
            head: 0,
            tail: 0,
        }
    }

    /// Borrows this batch for production only when no retained value remains.
    #[inline]
    pub fn fill(&mut self) -> Option<Fill<'_, T, N>> {
        if !self.is_empty() {
            return None;
        }
        self.head = 0;
        self.tail = 0;
        Some(Fill { batch: self })
    }

    #[inline]
    pub fn pop_front(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let index = self.head;
        self.head += 1;
        Some(unsafe { self.slots[index].assume_init_read() })
    }

    /// Pops the front value while exclusively reserving its original position.
    ///
    /// Dropping the vacancy commits the pop. Calling [`FrontVacant::restore`]
    /// rolls it back without changing the order of the remaining values.
    #[inline]
    pub fn pop_front_reserved(&mut self) -> Option<(T, FrontVacant<'_, T, N>)> {
        let value = self.pop_front()?;
        Some((value, FrontVacant { batch: self }))
    }

    #[inline]
    pub fn clear(&mut self) {
        while let Some(value) = self.pop_front() {
            drop(value);
        }
        self.head = 0;
        self.tail = 0;
    }

    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.tail - self.head
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

impl<T, const N: usize> Fill<'_, T, N> {
    #[inline]
    pub fn vacant_entry(&mut self) -> Option<Vacant<'_, T, N>> {
        if self.batch.tail == N {
            None
        } else {
            Some(Vacant { batch: self.batch })
        }
    }
}

impl<T, const N: usize> Vacant<'_, T, N> {
    #[inline]
    pub fn insert(self, value: T) {
        let index = self.batch.tail;
        self.batch.slots[index].write(value);
        self.batch.tail = index + 1;
    }
}

impl<T, const N: usize> FrontVacant<'_, T, N> {
    #[inline]
    pub fn restore(self, value: T) {
        self.batch.head -= 1;
        self.batch.slots[self.batch.head].write(value);
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
