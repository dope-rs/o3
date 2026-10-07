use std::mem;

use crate::depot::{self, capacity};

/// Exact-capacity ring position and occupancy.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct State<W> {
    head: W,
    len: W,
}

impl State<u32> {
    pub(in crate::depot::queue::ring) const fn count(self) -> capacity::Count {
        capacity::Count::new(self.len)
    }
}

impl<W: depot::raw::Word> State<W> {
    pub(crate) const EMPTY: Self = Self {
        head: W::ZERO,
        len: W::ZERO,
    };

    pub(crate) fn head(self) -> usize {
        self.head.offset()
    }

    pub(crate) fn len(self) -> usize {
        self.len.offset()
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub(crate) fn is_full(self, capacity: usize) -> bool {
        self.len() == capacity
    }

    /// Returns a physical position for one in-range logical offset.
    /// # Safety
    /// `capacity != 0`, `head < capacity`, and `offset <= capacity`.
    pub(crate) unsafe fn position(self, capacity: usize, offset: usize) -> usize {
        let suffix = capacity - self.head();
        if offset < suffix {
            self.head() + offset
        } else {
            offset - suffix
        }
    }

    pub(crate) fn prefix(self, capacity: usize, len: usize) -> (usize, [usize; 2]) {
        let head = self.head();
        let first = len.min(capacity - head);
        (head, [first, len - first])
    }

    /// Claims one complete suffix and returns its two physical runs.
    /// # Safety
    /// `0 < capacity <= W::MAX`, `head < capacity`, `len <= capacity - self.len()`.
    pub(crate) unsafe fn extend(&mut self, capacity: usize, len: usize) -> (usize, [usize; 2]) {
        let tail = unsafe { self.position(capacity, self.len()) };
        let added = Self {
            head: unsafe { W::from_offset(tail) },
            len: unsafe { W::from_offset(len) },
        };
        self.len = unsafe { W::from_offset(self.len() + len) };
        added.prefix(capacity, len)
    }

    /// Releases a complete prefix and resets an empty ring's origin.
    /// # Safety
    /// `0 < capacity <= W::MAX`, `head < capacity`, `len <= self.len() <= capacity`.
    pub(crate) unsafe fn consume(&mut self, capacity: usize, len: usize) {
        let left = self.len() - len;
        let head = if left == 0 {
            0
        } else {
            unsafe { self.position(capacity, len) }
        };
        self.head = unsafe { W::from_offset(head) };
        self.len = unsafe { W::from_offset(left) };
    }

    /// Claims one end and increments occupancy in the retained direction.
    /// # Safety
    /// `len < capacity`, and `capacity` is representable by `W`.
    pub(in crate::depot) unsafe fn push<const FRONT: bool>(&mut self, capacity: usize) -> usize {
        if !FRONT {
            return unsafe { self.extend(capacity, 1) }.0;
        }
        let head = if self.head() == 0 {
            capacity - 1
        } else {
            self.head() - 1
        };
        self.head = unsafe { W::from_offset(head) };
        self.len = unsafe { W::from_offset(self.len() + 1) };
        head
    }

    /// Releases all entries while preserving the next physical slot.
    /// # Safety
    /// `0 < self.len() <= capacity <= W::MAX` and `head < capacity`.
    pub(in crate::depot::queue::ring) unsafe fn clear(&mut self, capacity: usize) {
        let head = unsafe { self.position(capacity, self.len()) };
        self.head = unsafe { W::from_offset(head) };
        self.len = W::ZERO;
    }

    /// Releases and returns the physical head position.
    /// # Safety
    /// `len != 0`, `head < capacity`, and `capacity` is representable by `W`.
    pub(crate) unsafe fn pop_front(&mut self, capacity: usize) -> usize {
        let index = self.head();
        let next = index + 1;
        self.head = unsafe { W::from_offset(if next == capacity { 0 } else { next }) };
        self.len = unsafe { W::from_offset(self.len() - 1) };
        index
    }
}

const _: () = {
    assert!(mem::size_of::<State<u32>>() == mem::size_of::<u64>());
    assert!(mem::size_of::<State<usize>>() == 2 * mem::size_of::<usize>());
};
