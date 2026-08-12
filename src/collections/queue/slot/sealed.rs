use std::{cell, marker, mem, ops};

const NONE: u32 = u32::MAX;

pub struct ExclusiveMode;
pub struct SharedMode;

struct Node<T> {
    value: cell::UnsafeCell<mem::MaybeUninit<T>>,
    prev: cell::Cell<u32>,
    next: cell::Cell<u32>,
}

struct Nodes<T>(Box<[Node<T>]>);

impl<T> Nodes<T> {
    fn try_with_capacity(capacity: usize) -> Result<Self, crate::collections::AllocationError> {
        assert!(
            u32::try_from(capacity).is_ok(),
            "slot queue capacity overflow"
        );
        Ok(Self(crate::collections::try_box_with(capacity, |index| {
            Node::vacant(index as u32)
        })?))
    }

    fn is_vacant(&self, index: usize) -> bool {
        self.get(index)
            .is_some_and(|entry| entry.prev.get() == index as u32)
    }
}

impl<T> ops::Deref for Nodes<T> {
    type Target = [Node<T>];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Node<T> {
    fn vacant(index: u32) -> Self {
        Self {
            value: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
            prev: cell::Cell::new(index),
            next: cell::Cell::new(NONE),
        }
    }
}

#[derive(Clone, Copy)]
struct State {
    head: u32,
    tail: u32,
    len: usize,
}

#[repr(transparent)]
struct Links(cell::Cell<State>);

impl Links {
    fn empty() -> Self {
        Self(cell::Cell::new(State {
            head: NONE,
            tail: NONE,
            len: 0,
        }))
    }

    fn head(&self) -> Option<usize> {
        let head = self.0.get().head;
        (head != NONE).then_some(head as usize)
    }

    fn len(&self) -> usize {
        self.0.get().len
    }

    fn is_empty(&self) -> bool {
        self.0.get().len == 0
    }

    /// # Safety
    /// `index` is a vacant entry in `nodes`.
    unsafe fn push_front<T>(&self, nodes: &Nodes<T>, index: usize) {
        let mut state = self.0.get();
        let entry = unsafe { nodes.get_unchecked(index) };
        entry.prev.set(NONE);
        entry.next.set(state.head);
        if state.head == NONE {
            state.tail = index as u32;
        } else {
            unsafe { nodes.get_unchecked(state.head as usize) }
                .prev
                .set(index as u32);
        }
        state.head = index as u32;
        state.len += 1;
        self.0.set(state);
    }

    /// # Safety
    /// `index` is a vacant entry in `nodes`.
    unsafe fn push_back<T>(&self, nodes: &Nodes<T>, index: usize) {
        let mut state = self.0.get();
        let entry = unsafe { nodes.get_unchecked(index) };
        entry.prev.set(state.tail);
        entry.next.set(NONE);
        if state.tail == NONE {
            state.head = index as u32;
        } else {
            unsafe { nodes.get_unchecked(state.tail as usize) }
                .next
                .set(index as u32);
        }
        state.tail = index as u32;
        state.len += 1;
        self.0.set(state);
    }

    /// # Safety
    /// `index` is an occupied entry in `nodes`.
    unsafe fn remove<T>(&self, nodes: &Nodes<T>, index: usize) {
        let mut state = self.0.get();
        let entry = unsafe { nodes.get_unchecked(index) };
        let prev = entry.prev.get();
        let next = entry.next.get();
        entry.prev.set(index as u32);
        entry.next.set(NONE);
        if prev == NONE {
            state.head = next;
        } else {
            unsafe { nodes.get_unchecked(prev as usize) }.next.set(next);
        }
        if next == NONE {
            state.tail = prev;
        } else {
            unsafe { nodes.get_unchecked(next as usize) }.prev.set(prev);
        }
        state.len -= 1;
        self.0.set(state);
    }
}

pub struct Core<T, Mode> {
    entries: Nodes<T>,
    links: Links,
    access: marker::PhantomData<fn() -> Mode>,
    _thread: crate::ThreadBound,
}

impl<T, Mode> Core<T, Mode> {
    pub(super) fn try_with_capacity(
        capacity: usize,
    ) -> Result<Self, crate::collections::AllocationError> {
        Ok(Self {
            entries: Nodes::try_with_capacity(capacity)?,
            links: Links::empty(),
            access: marker::PhantomData,
            _thread: crate::ThreadBound::NEW,
        })
    }

    pub(super) fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn len(&self) -> usize {
        self.links.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// # Safety
    /// `index` is in bounds and vacant, with no live reference into its value.
    unsafe fn push_front_unchecked(&self, index: usize, value: T) {
        debug_assert!(self.entries.is_vacant(index));
        let entry = unsafe { self.entries.get_unchecked(index) };
        unsafe { (*entry.value.get()).write(value) };
        unsafe { self.links.push_front(&self.entries, index) };
    }

    /// # Safety
    /// `index` is in bounds and vacant, with no live reference into its value.
    unsafe fn push_back_unchecked(&self, index: usize, value: T) {
        debug_assert!(self.entries.is_vacant(index));
        let entry = unsafe { self.entries.get_unchecked(index) };
        unsafe { (*entry.value.get()).write(value) };
        unsafe { self.links.push_back(&self.entries, index) };
    }

    /// # Safety
    /// `index` is in bounds and occupied, with no live reference into its value.
    unsafe fn remove_unchecked(&self, index: usize) -> T {
        debug_assert!(
            self.entries
                .get(index)
                .is_some_and(|entry| entry.prev.get() != index as u32)
        );
        unsafe { self.links.remove(&self.entries, index) };
        unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_read() }
    }
}

impl<T> Core<T, ExclusiveMode> {
    pub(super) fn write(&mut self) -> Write<'_, T> {
        Write { core: self }
    }

    pub(super) fn front(&self) -> Option<&T> {
        let index = self.links.head()?;
        Some(unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_ref() })
    }

    pub(super) fn front_key_value(&self) -> Option<(usize, &T)> {
        let index = self.links.head()?;
        let value = unsafe { (*self.entries.get_unchecked(index).value.get()).assume_init_ref() };
        Some((index, value))
    }
}

impl<T: Copy> Core<T, SharedMode> {
    pub(super) fn shared(&self) -> Shared<'_, T> {
        Shared { core: self }
    }
}

pub struct Write<'queue, T> {
    core: &'queue mut Core<T, ExclusiveMode>,
}

impl<'queue, T> Write<'queue, T> {
    pub(super) fn vacant(self, index: usize) -> Option<Vacancy<'queue, T>> {
        if !self.core.entries.is_vacant(index) {
            return None;
        }
        Some(Vacancy {
            core: self.core,
            index,
        })
    }

    pub(super) fn push_back(self, index: usize, value: T) -> Result<(), T> {
        if !self.core.entries.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.core.push_back_unchecked(index, value) };
        Ok(())
    }

    pub(super) fn push_front(self, index: usize, value: T) -> Result<(), T> {
        if !self.core.entries.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.core.push_front_unchecked(index, value) };
        Ok(())
    }

    pub(super) fn refresh_back(self, index: usize, value: T) -> Result<Option<T>, T> {
        let Some(entry) = self.core.entries.get(index) else {
            return Err(value);
        };
        let occupied = entry.prev.get() != index as u32;
        let previous = occupied.then(|| unsafe { self.core.remove_unchecked(index) });
        unsafe { self.core.push_back_unchecked(index, value) };
        Ok(previous)
    }

    pub(super) fn pop_front(self) -> Option<T> {
        let index = self.core.links.head()?;
        Some(unsafe { self.core.remove_unchecked(index) })
    }

    pub(super) fn pop_front_key_value(self) -> Option<(usize, T)> {
        let index = self.core.links.head()?;
        Some((index, unsafe { self.core.remove_unchecked(index) }))
    }

    pub(super) fn front_entry(self) -> Option<Occupied<'queue, T>> {
        let index = self.core.links.head()?;
        Some(Occupied {
            core: self.core,
            index,
        })
    }

    pub(super) fn remove(self, index: usize) -> Option<T> {
        let entry = self.core.entries.get(index)?;
        if entry.prev.get() == index as u32 {
            return None;
        }
        Some(unsafe { self.core.remove_unchecked(index) })
    }

    pub(super) fn remove_if(self, index: usize, predicate: impl FnOnce(&T) -> bool) -> Option<T> {
        let entry = self.core.entries.get(index)?;
        if entry.prev.get() == index as u32 {
            return None;
        }
        let value = unsafe { (*entry.value.get()).assume_init_ref() };
        if !predicate(value) {
            return None;
        }
        Some(unsafe { self.core.remove_unchecked(index) })
    }

    pub(super) fn clear(self) {
        while let Some(index) = self.core.links.head() {
            drop(unsafe { self.core.remove_unchecked(index) });
        }
    }
}

pub struct Shared<'queue, T: Copy> {
    core: &'queue Core<T, SharedMode>,
}

impl<T: Copy> Shared<'_, T> {
    pub(super) fn push_back(self, index: usize, value: T) -> Result<(), T> {
        if !self.core.entries.is_vacant(index) {
            return Err(value);
        }
        unsafe { self.core.push_back_unchecked(index, value) };
        Ok(())
    }

    pub(super) fn pop_front(self) -> Option<T> {
        let index = self.core.links.head()?;
        Some(unsafe { self.core.remove_unchecked(index) })
    }

    pub(super) fn remove(self, index: usize) -> Option<T> {
        let entry = self.core.entries.get(index)?;
        if entry.prev.get() == index as u32 {
            return None;
        }
        Some(unsafe { self.core.remove_unchecked(index) })
    }

    pub(super) fn remove_if(self, index: usize, predicate: impl FnOnce(&T) -> bool) -> Option<T> {
        let entry = self.core.entries.get(index)?;
        if entry.prev.get() == index as u32 {
            return None;
        }
        let value = unsafe { (*entry.value.get()).assume_init_ref() };
        if !predicate(value) {
            return None;
        }
        Some(unsafe { self.core.remove_unchecked(index) })
    }
}

pub struct Vacancy<'queue, T> {
    core: &'queue mut Core<T, ExclusiveMode>,
    index: usize,
}

impl<T> Vacancy<'_, T> {
    pub(super) fn push_front(self, value: T) {
        unsafe { self.core.push_front_unchecked(self.index, value) };
    }

    pub(super) fn push_back(self, value: T) {
        unsafe { self.core.push_back_unchecked(self.index, value) };
    }
}

pub struct Occupied<'queue, T> {
    core: &'queue mut Core<T, ExclusiveMode>,
    index: usize,
}

impl<T> Occupied<'_, T> {
    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn remove(self) -> T {
        unsafe { self.core.remove_unchecked(self.index) }
    }
}
