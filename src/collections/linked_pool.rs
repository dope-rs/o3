use std::cell::{Cell, UnsafeCell};
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr::NonNull;

use crate::marker::ThreadBound;

use super::ClearGuard;

const NONE: u32 = u32::MAX;

struct Node<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    next: Cell<u32>,
}

pub struct LinkedPool<T> {
    nodes: Box<[Node<T>]>,
    free: Cell<u32>,
    available: Cell<usize>,
    _pin: PhantomPinned,
    _thread: ThreadBound,
}

pub struct LinkedPoolChain<'pool, T> {
    pool: NonNull<LinkedPool<T>>,
    head: u32,
    tail: u32,
    len: usize,
    lifetime: PhantomData<&'pool LinkedPool<T>>,
}

impl<T> LinkedPool<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "linked pool capacity overflow"
        );
        Self {
            nodes: (0..capacity)
                .map(|index| Node {
                    value: UnsafeCell::new(MaybeUninit::uninit()),
                    next: Cell::new(if index + 1 == capacity {
                        NONE
                    } else {
                        index as u32 + 1
                    }),
                })
                .collect(),
            free: Cell::new(if capacity == 0 { NONE } else { 0 }),
            available: Cell::new(capacity),
            _pin: PhantomPinned,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn chain(self: Pin<&Self>) -> LinkedPoolChain<'_, T> {
        LinkedPoolChain {
            pool: NonNull::from(self.get_ref()),
            head: NONE,
            tail: NONE,
            len: 0,
            lifetime: PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.nodes.len()
    }

    pub fn len(&self) -> usize {
        self.nodes.len() - self.available.get()
    }

    pub fn available(&self) -> usize {
        self.available.get()
    }

    pub fn is_empty(&self) -> bool {
        self.available.get() == self.nodes.len()
    }

    pub fn is_full(&self) -> bool {
        self.available.get() == 0
    }

    fn allocate(&self, value: T) -> u32 {
        let index = self.free.get();
        debug_assert_ne!(index, NONE);
        let node = unsafe { self.nodes.get_unchecked(index as usize) };
        self.free.set(node.next.get());
        self.available.set(self.available.get() - 1);
        node.next.set(NONE);
        unsafe { (*node.value.get()).write(value) };
        index
    }

    fn release(&self, index: u32) -> T {
        let node = unsafe { self.nodes.get_unchecked(index as usize) };
        let value = unsafe { (*node.value.get()).assume_init_read() };
        node.next.set(self.free.get());
        self.free.set(index);
        self.available.set(self.available.get() + 1);
        value
    }
}

impl<T> LinkedPoolChain<'_, T> {
    fn pool(&self) -> &LinkedPool<T> {
        unsafe { self.pool.as_ref() }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn front(&self) -> Option<&T> {
        if self.head == NONE {
            return None;
        }
        let node = unsafe { self.pool().nodes.get_unchecked(self.head as usize) };
        Some(unsafe { &*node.value.get().cast::<T>() })
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        if self.pool().is_full() {
            return Err(value);
        }
        let index = self.pool().allocate(value);
        if self.tail == NONE {
            self.head = index;
        } else {
            let node = unsafe { self.pool().nodes.get_unchecked(self.tail as usize) };
            node.next.set(index);
        }
        self.tail = index;
        self.len += 1;
        Ok(())
    }

    pub fn push_front(&mut self, value: T) -> Result<(), T> {
        if self.pool().is_full() {
            return Err(value);
        }
        let index = self.pool().allocate(value);
        let node = unsafe { self.pool().nodes.get_unchecked(index as usize) };
        node.next.set(self.head);
        self.head = index;
        if self.tail == NONE {
            self.tail = index;
        }
        self.len += 1;
        Ok(())
    }

    pub fn pop_front(&mut self) -> Option<T> {
        let index = self.head;
        if index == NONE {
            return None;
        }
        let node = unsafe { self.pool().nodes.get_unchecked(index as usize) };
        let next = node.next.get();
        self.head = next;
        self.len -= 1;
        if next == NONE {
            self.tail = NONE;
        }
        Some(self.pool().release(index))
    }

    pub fn clear(&mut self) {
        ClearGuard::run(self, Self::clear_remaining);
    }

    fn clear_remaining(&mut self) {
        while let Some(value) = self.pop_front() {
            drop(value);
        }
    }
}

impl<T> Drop for LinkedPoolChain<'_, T> {
    fn drop(&mut self) {
        self.clear();
    }
}
