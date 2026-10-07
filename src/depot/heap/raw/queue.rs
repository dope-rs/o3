//! Raw one-allocation storage binding heap positions, values, hooks, and free indices.

use std::{mem, ptr};

use crate::{
    alloc,
    depot::{capacity, heap, queue::links},
};

pub(in crate::depot) struct Node<V> {
    hook: links::Hook,
    value: mem::MaybeUninit<V>,
}

impl<V> Node<V> {
    fn vacant(index: usize, capacity: usize) -> Self {
        let next = if index + 1 == capacity {
            links::Index::END
        } else {
            (index + 1) as u32
        };
        Self {
            hook: links::Hook::vacant(index as u32, next),
            value: mem::MaybeUninit::uninit(),
        }
    }
}

pub(in crate::depot) struct Queue<P: Ord, V> {
    storage: Storage<P, V>,
}

impl<P: Ord, V> Queue<P, V> {
    pub(in crate::depot) fn try_new(capacity: capacity::Count) -> Result<Self, heap::Error> {
        Ok(Self {
            storage: heap::Capacity::try_new(capacity)?
                .allocate()?
                .enumerate(|index| Node::vacant(index, capacity.get())),
        })
    }

    pub(in crate::depot) const fn capacity(&self) -> usize {
        self.storage.extent().count().get()
    }
    pub(in crate::depot) const fn len(&self) -> usize {
        self.storage.state as usize
    }
    pub(in crate::depot) const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(in crate::depot) fn min(&self) -> Option<(links::Index, &P)> {
        let (key, priority) = heap::raw::Store::peek(self)?;
        // SAFETY: every initialized resident of this store holds a compact non-sentinel index.
        Some((unsafe { links::Index::new_usize(key.get()) }, priority))
    }

    /// # Safety
    /// `index` is occupied by this store; any supplied `position` belongs to it.
    unsafe fn position(&self, index: links::Index, position: Option<links::Index>) -> usize {
        match position {
            Some(position) => position.get(),
            None => (unsafe { *self.storage.tail().get_unchecked(index.get()) }) as usize,
        }
    }

    /// # Safety
    /// `index` is occupied by this store; any supplied `position` belongs to it.
    pub(in crate::depot) unsafe fn priority(
        &self,
        index: links::Index,
        position: Option<links::Index>,
    ) -> &P {
        let position = unsafe { self.position(index, position) };
        let entry = unsafe {
            self.storage
                .as_ref()
                .get_unchecked(position)
                .assume_init_ref()
        };
        &entry.priority
    }

    /// # Safety
    /// `index` must be occupied by this store and its returned borrow unique.
    pub(in crate::depot) unsafe fn value_mut(&mut self, index: links::Index) -> &mut V {
        let node = unsafe {
            self.storage
                .values_mut()
                .get_unchecked_mut(index.raw() as usize)
        };
        unsafe { node.value.assume_init_mut() }
    }

    /// # Safety
    /// `index` must be a vacant resident with uninitialized value storage.
    pub(in crate::depot) unsafe fn write(&mut self, index: links::Index, value: V) {
        let node = unsafe {
            self.storage
                .values_mut()
                .get_unchecked_mut(index.raw() as usize)
        };
        node.value.write(value);
    }

    /// # Safety
    /// `index` is an admitted absent resident. Initialize its value and publish
    /// its FIFO membership before releasing the enclosing exclusive queue loan.
    pub(in crate::depot) unsafe fn insert(&mut self, index: links::Index, priority: P) {
        let key = unsafe { heap::Direct::from_index(index.raw() as usize) };
        unsafe { heap::Vacancy::new(self, key) }
            .prepare(priority)
            .store();
    }

    /// # Safety
    /// `index` is occupied by this store; any supplied `position` belongs to it.
    pub(in crate::depot) unsafe fn remove(
        &mut self,
        index: links::Index,
        position: Option<links::Index>,
    ) -> (P, V) {
        let position = unsafe { self.position(index, position) };
        let (vacant, priority) = unsafe { heap::Entry::new(self, position, ()) }.remove_slot();
        let node = unsafe {
            vacant
                .owner
                .storage
                .values_mut()
                .get_unchecked_mut(vacant.value.get())
        };
        let value = unsafe { node.value.assume_init_read() };
        (priority, value)
    }
}

impl<P: Ord, V> Drop for Queue<P, V> {
    fn drop(&mut self) {
        if mem::needs_drop::<P>() || mem::needs_drop::<V>() {
            for position in 0..self.len() {
                // SAFETY: terminal ownership retains the initialized heap prefix
                // and its bijection to live residents until both are destroyed.
                let entry = unsafe { self.storage.as_mut().get_unchecked_mut(position) };
                let index = unsafe { entry.assume_init_ref() }.identity.get();
                unsafe { entry.assume_init_drop() };
                let node = unsafe { self.storage.values_mut().get_unchecked_mut(index) };
                unsafe { node.value.assume_init_drop() };
            }
        }
    }
}

unsafe impl<P: Ord, V> heap::raw::Store for Queue<P, V> {
    type Key = heap::Direct;
    type Priority = P;
    type Position = u32;
    type Storage = heap::Capacity<heap::Stored<heap::Direct, P>, u32, Node<V>>;
    type Extra = ptr::NonNull<Node<V>>;

    fn precedes(_: &Self::Key, _: &Self::Key) -> bool {
        false
    }

    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &mut self.storage
    }
}

unsafe impl<P: Ord, V> crate::depot::raw::Hooks for Queue<P, V> {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        &unsafe { self.storage.values().get_unchecked(index.raw() as usize) }.hook
    }
}

/// The single allocation binds heap priorities and resident payload nodes.
type Storage<P, V> = alloc::raw::Block<
    heap::Capacity<heap::Stored<heap::Direct, P>, u32, Node<V>>,
    u32,
    ptr::NonNull<Node<V>>,
>;
