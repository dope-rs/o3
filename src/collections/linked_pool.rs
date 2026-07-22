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

/// Fixed node storage for scoped chains that borrow a pinned outer owner.
pub struct LinkedPool<T> {
    nodes: NodePool<T>,
    _pin: PhantomPinned,
}

/// A FIFO chain that cannot outlive its pinned [`LinkedPool`].
pub struct LinkedPoolChain<'pool, T> {
    nodes: NonNull<NodePool<T>>,
    state: ChainState,
    lifetime: PhantomData<&'pool NodePool<T>>,
}

/// Fixed node storage and persistent FIFO lanes under one movable owner.
///
/// Unlike [`LinkedPoolChain`], lanes are indices rather than self-references.
/// This makes the arena suitable for long-lived state that owns both shared
/// storage and every lane using it.
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

    fn capacity(&self) -> usize {
        self.nodes.len()
    }

    fn len(&self) -> usize {
        self.nodes.len() - self.available.get()
    }

    fn available(&self) -> usize {
        self.available.get()
    }

    fn is_empty(&self) -> bool {
        self.available.get() == self.nodes.len()
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

impl<T> LinkedPool<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: NodePool::with_capacity(capacity),
            _pin: PhantomPinned,
        }
    }

    pub fn chain(self: Pin<&Self>) -> LinkedPoolChain<'_, T> {
        LinkedPoolChain {
            nodes: NonNull::from(&self.get_ref().nodes),
            state: ChainState::EMPTY,
            lifetime: PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn available(&self) -> usize {
        self.nodes.available()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }
}

impl<T> LinkedPoolChain<'_, T> {
    fn nodes(&self) -> &NodePool<T> {
        unsafe { self.nodes.as_ref() }
    }

    pub fn len(&self) -> usize {
        self.state.len
    }

    pub fn is_empty(&self) -> bool {
        self.state.len == 0
    }

    pub fn front(&self) -> Option<&T> {
        self.nodes().front(&self.state)
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        let nodes = self.nodes;
        unsafe { nodes.as_ref() }.push_back(&mut self.state, value)
    }

    pub fn push_front(&mut self, value: T) -> Result<(), T> {
        let nodes = self.nodes;
        unsafe { nodes.as_ref() }.push_front(&mut self.state, value)
    }

    pub fn pop_front(&mut self) -> Option<T> {
        let nodes = self.nodes;
        unsafe { nodes.as_ref() }.pop_front(&mut self.state)
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

impl<T> LinkedArena<T> {
    pub fn with_capacity(capacity: usize, lanes: usize) -> Self {
        assert!(lanes > 0, "linked arena lane count must be positive");
        Self {
            nodes: NodePool::with_capacity(capacity),
            lanes: vec![ChainState::EMPTY; lanes].into_boxed_slice(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn available(&self) -> usize {
        self.nodes.available()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
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

    pub fn clear(&mut self) {
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
