use std::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
    ops::Deref,
};

const NONE: u32 = u32::MAX;

struct Node<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    prev: Cell<u32>,
    next: Cell<u32>,
}

struct Nodes<T>(Box<[Node<T>]>);

impl<T> Nodes<T> {
    fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "slot queue capacity overflow"
        );
        Self((0..capacity as u32).map(Node::vacant).collect())
    }

    fn is_vacant(&self, index: usize) -> bool {
        self.get(index)
            .is_some_and(|entry| entry.prev.get() == index as u32)
    }

    fn grow_to(&mut self, capacity: usize) {
        use crate::collections::BoxSliceGrowth;
        let old_capacity = self.len();
        assert!(capacity >= old_capacity, "slot queue cannot shrink");
        assert!(
            u32::try_from(capacity).is_ok(),
            "slot queue capacity overflow"
        );
        if capacity == old_capacity {
            return;
        }

        let mut entries = BoxSliceGrowth::take(&mut self.0);
        entries.reserve_exact(capacity - old_capacity);
        for index in old_capacity..capacity {
            entries.push(Node::vacant(index as u32));
        }
    }
}

impl<T> Deref for Nodes<T> {
    type Target = [Node<T>];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Node<T> {
    fn vacant(index: u32) -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            prev: Cell::new(index),
            next: Cell::new(NONE),
        }
    }
}

#[derive(Clone, Copy)]
struct State {
    head: u32,
    tail: u32,
    len: usize,
}

impl State {
    const EMPTY: Self = Self {
        head: NONE,
        tail: NONE,
        len: 0,
    };

    fn len(self) -> usize {
        self.len
    }

    fn is_empty(self) -> bool {
        self.len == 0
    }
}

struct Core<T> {
    entries: Nodes<T>,
    state: Cell<State>,
    _thread: crate::ThreadBound,
}

impl<T> Core<T> {
    fn with_capacity(capacity: usize) -> Self {
        use crate::ThreadBound;
        Self {
            entries: Nodes::with_capacity(capacity),
            state: Cell::new(State::EMPTY),
            _thread: ThreadBound::NEW,
        }
    }

    fn push_back(&self, index: usize, value: T) -> Result<(), T> {
        if !self.entries.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.push_back_unchecked(index, value) };
        Ok(())
    }

    fn push_front(&self, index: usize, value: T) -> Result<(), T> {
        if !self.entries.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.push_front_unchecked(index, value) };
        Ok(())
    }

    fn refresh_back(&self, index: usize, value: T) -> Result<Option<T>, T> {
        let Some(entry) = self.entries.get(index) else {
            return Err(value);
        };
        let previous =
            (entry.prev.get() != index as u32).then(|| unsafe { self.remove_unchecked(index) });
        unsafe { self.push_back_unchecked(index, value) };
        Ok(previous)
    }

    /// # Safety
    /// `index` is in bounds and vacant.
    unsafe fn push_front_unchecked(&self, index: usize, value: T) {
        debug_assert!(self.entries.is_vacant(index));
        let entry = unsafe { self.entries.get_unchecked(index) };
        unsafe { (*entry.value.get()).write(value) };
        unsafe { self.link_front(index) };
    }

    /// # Safety
    /// `index` is in bounds and vacant.
    unsafe fn push_back_unchecked(&self, index: usize, value: T) {
        debug_assert!(self.entries.is_vacant(index));
        let entry = unsafe { self.entries.get_unchecked(index) };
        unsafe { (*entry.value.get()).write(value) };
        unsafe { self.link_back(index) };
    }

    unsafe fn link_front(&self, index: usize) {
        let mut state = self.state.get();
        let entry = unsafe { self.entries.get_unchecked(index) };
        entry.prev.set(NONE);
        entry.next.set(state.head);
        if state.head == NONE {
            state.tail = index as u32;
        } else {
            unsafe { self.entries.get_unchecked(state.head as usize) }
                .prev
                .set(index as u32);
        }
        state.head = index as u32;
        state.len += 1;
        self.state.set(state);
    }

    unsafe fn link_back(&self, index: usize) {
        let mut state = self.state.get();
        let entry = unsafe { self.entries.get_unchecked(index) };
        entry.prev.set(state.tail);
        entry.next.set(NONE);
        if state.tail == NONE {
            state.head = index as u32;
        } else {
            unsafe { self.entries.get_unchecked(state.tail as usize) }
                .next
                .set(index as u32);
        }
        state.tail = index as u32;
        state.len += 1;
        self.state.set(state);
    }

    fn pop_front(&self) -> Option<T> {
        self.pop_front_key_value().map(|(_, value)| value)
    }

    fn front(&self) -> Option<&T> {
        let index = (self.state.get().head != NONE).then_some(self.state.get().head as usize)?;
        let value = unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_ref() };
        Some(value)
    }

    fn front_key_value(&self) -> Option<(usize, &T)> {
        let index = (self.state.get().head != NONE).then_some(self.state.get().head as usize)?;
        let value = unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_ref() };
        Some((index, value))
    }

    fn pop_front_key_value(&self) -> Option<(usize, T)> {
        let index = (self.state.get().head != NONE).then_some(self.state.get().head as usize)?;
        Some((index, unsafe { self.remove_unchecked(index) }))
    }

    fn remove(&self, index: usize) -> Option<T> {
        let entry = self.entries.get(index)?;
        if entry.prev.get() == index as u32 {
            return None;
        }
        Some(unsafe { self.remove_unchecked(index) })
    }

    /// # Safety
    /// `index` is in bounds and occupied. No reference returned by `front`
    /// may be live while this method runs.
    unsafe fn remove_unchecked(&self, index: usize) -> T {
        debug_assert!(
            self.entries
                .get(index)
                .is_some_and(|entry| entry.prev.get() != index as u32)
        );
        unsafe { self.unlink(index) };
        unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_read() }
    }

    unsafe fn unlink(&self, index: usize) {
        let mut state = self.state.get();
        let entry = unsafe { self.entries.get_unchecked(index) };
        let prev = entry.prev.get();
        let next = entry.next.get();
        entry.prev.set(index as u32);
        entry.next.set(NONE);
        if prev == NONE {
            state.head = next;
        } else {
            unsafe { self.entries.get_unchecked(prev as usize) }
                .next
                .set(next);
        }
        if next == NONE {
            state.tail = prev;
        } else {
            unsafe { self.entries.get_unchecked(next as usize) }
                .prev
                .set(prev);
        }
        state.len -= 1;
        self.state.set(state);
    }

    fn clear(&self) {
        while self.pop_front().is_some() {}
    }
}

pub struct Fifo<T = ()> {
    core: Core<T>,
}

pub struct Vacant<'a, T> {
    queue: &'a mut Fifo<T>,
    index: usize,
}

impl<T> Vacant<'_, T> {
    pub fn push_back(self, value: T) {
        unsafe { self.queue.core.push_back_unchecked(self.index, value) };
    }
}

impl<T> Fifo<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            core: Core::with_capacity(capacity),
        }
    }

    pub fn vacant_entry(&mut self, index: usize) -> Option<Vacant<'_, T>> {
        self.core
            .entries
            .is_vacant(index)
            .then_some(Vacant { queue: self, index })
    }

    pub fn push_back(&mut self, index: usize, value: T) -> Result<(), T> {
        self.core.push_back(index, value)
    }

    pub fn push_front(&mut self, index: usize, value: T) -> Result<(), T> {
        self.core.push_front(index, value)
    }

    pub fn refresh_back(&mut self, index: usize, value: T) -> Result<(), T> {
        self.core.refresh_back(index, value).map(drop)
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.core.pop_front()
    }

    pub fn front(&self) -> Option<&T> {
        self.core.front()
    }

    pub fn front_key_value(&self) -> Option<(usize, &T)> {
        self.core.front_key_value()
    }

    pub fn pop_front_key_value(&mut self) -> Option<(usize, T)> {
        self.core.pop_front_key_value()
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        self.core.remove(index)
    }

    pub fn clear(&mut self) {
        self.core.clear();
    }

    pub fn grow_to(&mut self, capacity: usize) {
        self.core.entries.grow_to(capacity);
    }

    pub fn capacity(&self) -> usize {
        self.core.entries.len()
    }

    pub fn len(&self) -> usize {
        self.core.state.get().len()
    }

    pub fn is_empty(&self) -> bool {
        self.core.state.get().is_empty()
    }
}

impl<T> Drop for Fifo<T> {
    fn drop(&mut self) {
        use crate::collections::ClearGuard;
        ClearGuard::run(self, Self::clear);
    }
}

/// A fixed-capacity indexed queue with shared mutation.
/// `Copy` values prevent user code from running while links change.
pub struct CellFifo<T: Copy = ()> {
    core: Core<T>,
}

impl<T: Copy> CellFifo<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            core: Core::with_capacity(capacity),
        }
    }

    pub fn push_back(&self, index: usize, value: T) -> Result<(), T> {
        self.core.push_back(index, value)
    }

    pub fn pop_front(&self) -> Option<T> {
        self.core.pop_front()
    }

    pub fn remove(&self, index: usize) -> Option<T> {
        self.core.remove(index)
    }

    pub fn grow_to(&mut self, capacity: usize) {
        self.core.entries.grow_to(capacity);
    }

    pub fn is_empty(&self) -> bool {
        self.core.state.get().is_empty()
    }
}
