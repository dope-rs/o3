use std::mem::MaybeUninit;

use crate::marker::ThreadBound;

use super::ClearGuard;

const NONE: u32 = u32::MAX;

struct Node<T> {
    value: MaybeUninit<T>,
    next: u32,
}

struct NodePool<T> {
    nodes: Box<[MaybeUninit<Node<T>>]>,
    free: u32,
    initialized: u32,
    live: u32,
}

#[derive(Clone, Copy)]
struct ChainState {
    head: u32,
    tail: u32,
    len: u32,
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
    _thread: ThreadBound,
}

/// Fixed node storage shared by persistent LIFO lanes.
///
/// Nodes are initialized only when first used. Moving a value between lanes or
/// returning it to the pool never allocates.
pub struct StackArena<T> {
    nodes: NodePool<T>,
    lanes: Box<[u32]>,
}

/// Values removed from one [`StackArena`] lane.
///
/// Dropping the iterator releases every value that has not yet been yielded.
pub struct StackDrain<'a, T> {
    arena: &'a mut StackArena<T>,
    lane: usize,
}

impl<T> NodePool<T> {
    fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "linked node capacity overflow"
        );
        Self {
            nodes: Box::<[Node<T>]>::new_uninit_slice(capacity),
            free: NONE,
            initialized: 0,
            live: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.live as usize == self.nodes.len()
    }

    fn capacity(&self) -> usize {
        self.nodes.len()
    }

    fn available(&self) -> usize {
        self.nodes.len() - self.live as usize
    }

    fn front<'a>(&'a self, state: &ChainState) -> Option<&'a T> {
        self.value(state.head)
    }

    fn front_mut<'a>(&'a mut self, state: &ChainState) -> Option<&'a mut T> {
        self.value_mut(state.head)
    }

    fn value(&self, index: u32) -> Option<&T> {
        if index == NONE {
            return None;
        }
        let node = unsafe { self.node(index) };
        Some(unsafe { node.value.assume_init_ref() })
    }

    fn value_mut(&mut self, index: u32) -> Option<&mut T> {
        if index == NONE {
            return None;
        }
        let node = unsafe { self.node_mut(index) };
        Some(unsafe { node.value.assume_init_mut() })
    }

    fn push_back(&mut self, state: &mut ChainState, value: T) -> Result<(), T> {
        let index = self.allocate(value)?;
        if state.tail == NONE {
            state.head = index;
        } else {
            unsafe { self.node_mut(state.tail) }.next = index;
        }
        state.tail = index;
        state.len += 1;
        Ok(())
    }

    fn push_front(&mut self, state: &mut ChainState, value: T) -> Result<(), T> {
        let head = state.head;
        let index = self.allocate(value)?;
        unsafe { self.node_mut(index) }.next = head;
        state.head = index;
        if state.tail == NONE {
            state.tail = index;
        }
        state.len += 1;
        Ok(())
    }

    fn pop_front(&mut self, state: &mut ChainState) -> Option<T> {
        let index = state.head;
        if index == NONE {
            return None;
        }
        let next = unsafe { self.node(index) }.next;
        state.head = next;
        state.len -= 1;
        if next == NONE {
            state.tail = NONE;
        }
        Some(unsafe { self.release(index) })
    }

    fn push(&mut self, head: &mut u32, value: T) -> Result<(), T> {
        let previous = *head;
        let index = self.allocate(value)?;
        unsafe { self.node_mut(index) }.next = previous;
        *head = index;
        Ok(())
    }

    fn pop(&mut self, head: &mut u32) -> Option<T> {
        let index = *head;
        if index == NONE {
            return None;
        }
        *head = unsafe { self.node(index) }.next;
        Some(unsafe { self.release(index) })
    }

    fn allocate(&mut self, value: T) -> Result<u32, T> {
        if self.is_full() {
            return Err(value);
        }
        let index = if self.free == NONE {
            let index = self.initialized;
            debug_assert!((index as usize) < self.nodes.len());
            self.initialized += 1;
            self.nodes[index as usize].write(Node {
                value: MaybeUninit::new(value),
                next: NONE,
            });
            index
        } else {
            let index = self.free;
            let next = {
                let node = unsafe { self.node_mut(index) };
                let next = node.next;
                node.value.write(value);
                node.next = NONE;
                next
            };
            self.free = next;
            index
        };
        self.live += 1;
        Ok(index)
    }

    /// The caller has unlinked `index`, so its value is initialized and the
    /// node can be returned to this pool before the value is dropped.
    unsafe fn release(&mut self, index: u32) -> T {
        let free = self.free;
        let node = unsafe { self.node_mut(index) };
        let value = unsafe { node.value.assume_init_read() };
        node.next = free;
        self.free = index;
        self.live -= 1;
        value
    }

    /// Indices below `initialized` contain initialized `Node<T>` metadata.
    unsafe fn node(&self, index: u32) -> &Node<T> {
        debug_assert!(index < self.initialized);
        unsafe { self.nodes.get_unchecked(index as usize).assume_init_ref() }
    }

    /// Exclusive arena access prevents aliases to node metadata or values.
    unsafe fn node_mut(&mut self, index: u32) -> &mut Node<T> {
        debug_assert!(index < self.initialized);
        unsafe {
            self.nodes
                .get_unchecked_mut(index as usize)
                .assume_init_mut()
        }
    }
}

impl<T> LinkedArena<T> {
    pub fn with_capacity(capacity: usize, lanes: usize) -> Self {
        Self {
            nodes: NodePool::with_capacity(capacity),
            lanes: vec![ChainState::EMPTY; lanes].into_boxed_slice(),
            _thread: ThreadBound::NEW,
        }
    }

    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }

    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    pub fn available(&self) -> usize {
        self.nodes.available()
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    pub fn lane_len(&self, lane: usize) -> usize {
        self.lanes[lane].len as usize
    }

    pub fn lane_is_empty(&self, lane: usize) -> bool {
        self.lanes.get(lane).is_none_or(|state| state.len == 0)
    }

    pub fn front(&self, lane: usize) -> Option<&T> {
        self.nodes.front(self.lanes.get(lane)?)
    }

    pub fn front_mut(&mut self, lane: usize) -> Option<&mut T> {
        self.nodes.front_mut(self.lanes.get(lane)?)
    }

    pub fn push_back(&mut self, lane: usize, value: T) -> Result<(), T> {
        let Some(state) = self.lanes.get_mut(lane) else {
            return Err(value);
        };
        self.nodes.push_back(state, value)
    }

    pub fn push_front(&mut self, lane: usize, value: T) -> Result<(), T> {
        let Some(state) = self.lanes.get_mut(lane) else {
            return Err(value);
        };
        self.nodes.push_front(state, value)
    }

    pub fn pop_front(&mut self, lane: usize) -> Option<T> {
        self.nodes.pop_front(self.lanes.get_mut(lane)?)
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

impl<T> StackArena<T> {
    pub fn with_capacity(capacity: usize, lanes: usize) -> Self {
        Self {
            nodes: NodePool::with_capacity(capacity),
            lanes: vec![NONE; lanes].into_boxed_slice(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }

    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    pub fn available(&self) -> usize {
        self.nodes.available()
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    pub fn lane_is_empty(&self, lane: usize) -> bool {
        self.lanes.get(lane).is_none_or(|head| *head == NONE)
    }

    pub fn push(&mut self, lane: usize, value: T) -> Result<(), T> {
        let Some(head) = self.lanes.get_mut(lane) else {
            return Err(value);
        };
        self.nodes.push(head, value)
    }

    pub fn pop(&mut self, lane: usize) -> Option<T> {
        self.nodes.pop(self.lanes.get_mut(lane)?)
    }

    pub fn drain(&mut self, lane: usize) -> StackDrain<'_, T> {
        StackDrain { arena: self, lane }
    }

    fn clear(&mut self) {
        ClearGuard::run(self, Self::clear_remaining);
    }

    fn clear_remaining(&mut self) {
        for lane in 0..self.lanes.len() {
            while let Some(value) = self.pop(lane) {
                drop(value);
            }
        }
    }
}

impl<T> Drop for StackArena<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T> Iterator for StackDrain<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.arena.pop(self.lane)
    }
}

impl<T> Drop for StackDrain<'_, T> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear_remaining);
    }
}

impl<T> StackDrain<'_, T> {
    fn clear_remaining(&mut self) {
        for value in self.by_ref() {
            drop(value);
        }
    }
}
