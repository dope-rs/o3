use std::mem;

use crate::collections;

const NONE: u32 = u32::MAX;

struct Node<T> {
    value: mem::MaybeUninit<T>,
    next: u32,
}

struct NodePool<T> {
    nodes: Box<[mem::MaybeUninit<Node<T>>]>,
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

    fn front<'a, T>(&self, nodes: &'a NodePool<T>) -> Option<&'a T> {
        nodes.value(self.head)
    }

    fn front_mut<'a, T>(&self, nodes: &'a mut NodePool<T>) -> Option<&'a mut T> {
        nodes.value_mut(self.head)
    }

    fn push_back<T>(&mut self, nodes: &mut NodePool<T>, value: T) -> Result<(), T> {
        let index = nodes.allocate(value)?;
        if self.tail == NONE {
            self.head = index;
        } else {
            unsafe { nodes.node_mut(self.tail) }.next = index;
        }
        self.tail = index;
        self.len += 1;
        Ok(())
    }

    fn push_front<T>(&mut self, nodes: &mut NodePool<T>, value: T) -> Result<(), T> {
        let index = nodes.allocate(value)?;
        unsafe { nodes.node_mut(index) }.next = self.head;
        self.head = index;
        if self.tail == NONE {
            self.tail = index;
        }
        self.len += 1;
        Ok(())
    }

    fn pop_front<T>(&mut self, nodes: &mut NodePool<T>) -> Option<T> {
        let index = self.head;
        if index == NONE {
            return None;
        }
        let next = unsafe { nodes.node(index) }.next;
        self.head = next;
        self.len -= 1;
        if next == NONE {
            self.tail = NONE;
        }
        Some(unsafe { nodes.release(index) })
    }
}

/// Fixed node storage and persistent FIFO lanes under one movable owner.
pub struct Linked<T> {
    nodes: NodePool<T>,
    lanes: Box<[ChainState]>,
    _thread: crate::ThreadBound,
}

/// A located FIFO entry which leaves its lane unchanged until consumed.
#[must_use]
pub struct LinkedFront<'a, T> {
    linked: &'a mut Linked<T>,
    lane: usize,
    index: u32,
}

/// Fixed node storage shared by persistent LIFO lanes.
/// Nodes initialize on first use and move between lanes without allocation.
pub struct Stack<T> {
    nodes: NodePool<T>,
    lanes: Box<[u32]>,
}

/// Values removed from one [`Stack`] lane.
///
/// Dropping the iterator releases every value that has not yet been yielded.
pub struct StackDrain<'a, T> {
    arena: &'a mut Stack<T>,
    lane: usize,
}

impl<T> NodePool<T> {
    fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(pool) => pool,
            Err(error) => error.abort(),
        }
    }

    fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        assert!(
            u32::try_from(capacity).is_ok(),
            "linked node capacity overflow"
        );
        Ok(Self {
            nodes: collections::BoxUninitExt::try_box_uninit(capacity)?,
            free: NONE,
            initialized: 0,
            live: 0,
        })
    }

    fn is_full(&self) -> bool {
        self.live as usize == self.nodes.len()
    }

    fn available(&self) -> usize {
        self.nodes.len() - self.live as usize
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
                value: mem::MaybeUninit::new(value),
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

impl<T> Linked<T> {
    pub fn with_capacity(capacity: usize, lanes: usize) -> Self {
        match Self::try_with_capacity(capacity, lanes) {
            Ok(arena) => arena,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(
        capacity: usize,
        lanes: usize,
    ) -> Result<Self, collections::AllocationError> {
        use crate::ThreadBound;
        Ok(Self {
            nodes: NodePool::try_with_capacity(capacity)?,
            lanes: collections::BoxSliceExt::try_box_with(lanes, |_| ChainState::EMPTY)?,
            _thread: ThreadBound::NEW,
        })
    }

    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }

    pub fn lane_len(&self, lane: usize) -> usize {
        self.lanes[lane].len as usize
    }

    pub fn lane_is_empty(&self, lane: usize) -> bool {
        self.lanes.get(lane).is_none_or(|state| state.len == 0)
    }

    pub fn front(&self, lane: usize) -> Option<&T> {
        self.lanes.get(lane)?.front(&self.nodes)
    }

    pub fn front_mut(&mut self, lane: usize) -> Option<&mut T> {
        self.lanes.get(lane)?.front_mut(&mut self.nodes)
    }

    pub fn front_entry(&mut self, lane: usize) -> Option<LinkedFront<'_, T>> {
        let index = self.lanes.get(lane)?.head;
        (index != NONE).then_some(LinkedFront {
            linked: self,
            lane,
            index,
        })
    }

    pub fn push_back(&mut self, lane: usize, value: T) -> Result<(), T> {
        let Some(state) = self.lanes.get_mut(lane) else {
            return Err(value);
        };
        state.push_back(&mut self.nodes, value)
    }

    pub fn push_front(&mut self, lane: usize, value: T) -> Result<(), T> {
        let Some(state) = self.lanes.get_mut(lane) else {
            return Err(value);
        };
        state.push_front(&mut self.nodes, value)
    }

    pub fn pop_front(&mut self, lane: usize) -> Option<T> {
        self.lanes.get_mut(lane)?.pop_front(&mut self.nodes)
    }

    fn clear(&mut self) {
        collections::ClearGuard::run(self, Self::clear_remaining);
    }

    fn clear_remaining(&mut self) {
        for lane in 0..self.lanes.len() {
            while let Some(value) = self.pop_front(lane) {
                drop(value);
            }
        }
    }
}

impl<T> LinkedFront<'_, T> {
    pub fn take(self) -> T {
        let state = &mut self.linked.lanes[self.lane];
        debug_assert_eq!(state.head, self.index);
        let next = unsafe { self.linked.nodes.node(self.index) }.next;
        state.head = next;
        state.len -= 1;
        if next == NONE {
            state.tail = NONE;
        }
        unsafe { self.linked.nodes.release(self.index) }
    }
}

impl<T> Drop for Linked<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T> Stack<T> {
    pub fn with_capacity(capacity: usize, lanes: usize) -> Self {
        Self {
            nodes: NodePool::with_capacity(capacity),
            lanes: vec![NONE; lanes].into_boxed_slice(),
        }
    }

    pub fn available(&self) -> usize {
        self.nodes.available()
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
        collections::ClearGuard::run(self, Self::clear_remaining);
    }

    fn clear_remaining(&mut self) {
        for lane in 0..self.lanes.len() {
            while let Some(value) = self.pop(lane) {
                drop(value);
            }
        }
    }
}

impl<T> Drop for Stack<T> {
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
        collections::ClearGuard::run(self, Self::clear_remaining);
    }
}

impl<T> StackDrain<'_, T> {
    fn clear_remaining(&mut self) {
        for value in self.by_ref() {
            drop(value);
        }
    }
}
