use std::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;

use crate::marker::ThreadBound;

use super::ClearGuard;

const NONE: u32 = u32::MAX;

struct Node<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    next: Cell<u32>,
}

struct NodePool<T> {
    nodes: Box<[Node<T>]>,
    free: Cell<u32>,
    available: Cell<usize>,
    _thread: ThreadBound,
}

#[derive(Clone, Copy)]
struct ChainState {
    head: u32,
    tail: u32,
    len: usize,
}

impl ChainState {
    const EMPTY: Self = Self {
        head: NONE,
        tail: NONE,
        len: 0,
    };
}

/// Fixed node storage and persistent FIFO lanes under one movable owner.
pub struct LinkedArena<T> {
    nodes: NodePool<T>,
    lanes: Box<[ChainState]>,
}

impl<T> NodePool<T> {
    fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "linked node capacity overflow"
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
            _thread: ThreadBound::NEW,
        }
    }

    fn is_full(&self) -> bool {
        self.available.get() == 0
    }

    fn front<'a>(&'a self, state: &ChainState) -> Option<&'a T> {
        if state.head == NONE {
            return None;
        }
        let node = unsafe { self.nodes.get_unchecked(state.head as usize) };
        Some(unsafe { &*node.value.get().cast::<T>() })
    }

    fn push_back(&self, state: &mut ChainState, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        let index = self.allocate(value);
        if state.tail == NONE {
            state.head = index;
        } else {
            let node = unsafe { self.nodes.get_unchecked(state.tail as usize) };
            node.next.set(index);
        }
        state.tail = index;
        state.len += 1;
        Ok(())
    }

    fn push_front(&self, state: &mut ChainState, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        let index = self.allocate(value);
        let node = unsafe { self.nodes.get_unchecked(index as usize) };
        node.next.set(state.head);
        state.head = index;
        if state.tail == NONE {
            state.tail = index;
        }
        state.len += 1;
        Ok(())
    }

    fn pop_front(&self, state: &mut ChainState) -> Option<T> {
        let index = state.head;
        if index == NONE {
            return None;
        }
        let node = unsafe { self.nodes.get_unchecked(index as usize) };
        let next = node.next.get();
        state.head = next;
        state.len -= 1;
        if next == NONE {
            state.tail = NONE;
        }
        Some(self.release(index))
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

impl<T> LinkedArena<T> {
    pub fn with_capacity(capacity: usize, lanes: usize) -> Self {
        assert!(lanes > 0, "linked arena lane count must be positive");
        Self {
            nodes: NodePool::with_capacity(capacity),
            lanes: vec![ChainState::EMPTY; lanes].into_boxed_slice(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }

    pub fn lane_len(&self, lane: usize) -> usize {
        self.lanes[lane].len
    }

    pub fn lane_is_empty(&self, lane: usize) -> bool {
        self.lanes[lane].len == 0
    }

    pub fn front(&self, lane: usize) -> Option<&T> {
        self.nodes.front(&self.lanes[lane])
    }

    pub fn push_back(&mut self, lane: usize, value: T) -> Result<(), T> {
        self.nodes.push_back(&mut self.lanes[lane], value)
    }

    pub fn push_front(&mut self, lane: usize, value: T) -> Result<(), T> {
        self.nodes.push_front(&mut self.lanes[lane], value)
    }

    pub fn pop_front(&mut self, lane: usize) -> Option<T> {
        self.nodes.pop_front(&mut self.lanes[lane])
    }

    fn clear(&mut self) {
        ClearGuard::run(self, Self::clear_remaining);
    }

    fn clear_remaining(&mut self) {
        for lane in 0..self.lanes.len() {
            while let Some(value) = self.pop_front(lane) {
                drop(value);
            }
        }
    }
}

impl<T> Drop for LinkedArena<T> {
    fn drop(&mut self) {
        self.clear();
    }
}
