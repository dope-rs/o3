//! FIFO values with an independent minimum-heap order.

use std::{mem, num, ops::ControlFlow, ptr};

use crate::alloc;

use crate::depot::{
    capacity,
    heap::{self, queue},
    queue::links,
};

const _: () = assert!(std::mem::size_of::<Min<(), ()>>() == 6 * std::mem::size_of::<usize>());

/// A bounded FIFO whose minimum priority can be removed independently.
pub struct Min<P: Ord, V> {
    pub(in crate::depot::heap::queue) storage: Storage<P, V>,
    pub(in crate::depot::heap::queue) ready: links::Chain<()>,
    pub(in crate::depot::heap::queue) free: links::Free,
}

impl<P: Ord, V> Min<P, V> {
    pub fn try_new(capacity: capacity::Count) -> Result<Self, heap::Error> {
        Ok(Self {
            storage: heap::Capacity::try_new(capacity)?
                .allocate()?
                .enumerate(|index| Node::vacant(index, capacity.get())),
            ready: links::Chain::new(),
            free: links::Free::seeded(capacity.get()),
        })
    }

    pub const fn capacity(&self) -> usize {
        self.storage.extent().count().get()
    }

    pub const fn len(&self) -> usize {
        self.storage.state as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn front(&mut self) -> Option<Occupied<'_, P, V>> {
        let index = self.ready.first()?;
        Some(Occupied {
            state: self,
            index,
            position: None,
        })
    }

    /// Visits only the admitted heads after acquiring one exclusive context.
    pub fn try_sweep<C, E, B>(
        &mut self,
        acquire: impl FnOnce() -> Result<C, E>,
        mut visit: impl FnMut(C, Occupied<'_, P, V>) -> ControlFlow<B, C>,
    ) -> Result<Option<ControlFlow<B, C>>, E> {
        let Some(count) = num::NonZero::new(self.len()) else {
            return Ok(None);
        };
        Ok(Some((0..count.get()).try_fold(acquire()?, |context, _| {
            // SAFETY: the initial FIFO cardinality admits this visit, and each
            // earlier exclusive entry can remove only its own current head.
            let index = unsafe { self.ready.front_key() };
            visit(
                context,
                Occupied {
                    state: self,
                    index,
                    position: None,
                },
            )
        })))
    }

    pub fn min(&self) -> Option<&P> {
        Some(heap::raw::Store::peek(self)?.1)
    }

    pub fn min_entry(&mut self) -> Option<Occupied<'_, P, V>> {
        let (key, _) = heap::raw::Store::peek(self)?;
        // SAFETY: the initialized root retains a compact non-sentinel resident index.
        let index = unsafe { links::Index::new_usize(key.get()) };
        Some(Occupied {
            state: self,
            index,
            // SAFETY: the nonempty store retains this index at its initialized root.
            position: Some(unsafe { links::Index::new_usize(0) }),
        })
    }

    pub fn vacant(&mut self) -> Option<Vacant<'_, P, V>> {
        let index = self.free.front()?;
        Some(Vacant { state: self, index })
    }
}

/// A linked value retained under an exclusive queue borrow.
#[must_use = "an occupied entry retains its exclusive owner borrow"]
pub struct Occupied<'a, P: Ord, V> {
    pub(in crate::depot::heap::queue) state: &'a mut queue::Min<P, V>,
    pub(in crate::depot::heap::queue) index: links::Index,
    pub(in crate::depot::heap::queue) position: Option<links::Index>,
}

impl<P: Ord, V> Occupied<'_, P, V> {
    pub fn priority(&self) -> &P {
        // SAFETY: the exclusive queue loan retains this live index and any known
        // heap position; payload access and FIFO relinking cannot change either.
        unsafe { self.state.priority(self.index, self.position) }
    }

    pub fn get_mut(&mut self) -> &mut V {
        // SAFETY: this entry retains the exact occupied index and the exclusive queue borrow.
        unsafe { self.state.value_mut(self.index) }
    }

    pub fn move_to_back(self) {
        if self.state.ready.last() == Some(self.index) {
            return;
        }
        // SAFETY: this entry retains the exact live index in the ready chain and
        // owns the exclusive queue borrow, so the relink preserves the topology.
        unsafe {
            self.state
                .ready
                .move_back(&self.state.ready, &self.state.storage, self.index)
        };
    }

    pub fn remove(self) -> (P, V) {
        // SAFETY: this entry retains the exact live index and any known heap
        // position under the same exclusive queue loan until removal begins.
        let output = unsafe { self.state.remove(self.index, self.position) };
        // SAFETY: the same index is still linked, so unlinking returns its exact vacancy hook.
        let vacant = unsafe { self.state.ready.unlink(&self.state.storage, self.index) };
        self.state.free.recycle(vacant);
        output
    }
}

/// The exact vacant slot reserved by an exclusive queue borrow.
#[must_use = "a vacant queue slot has not inserted a value"]
pub struct Vacant<'a, P: Ord, V> {
    pub(in crate::depot::heap::queue) state: &'a mut queue::Min<P, V>,
    pub(in crate::depot::heap::queue) index: links::Index,
}

impl<P: Ord, V> Vacant<'_, P, V> {
    pub fn push_back(self, priority: P, value: V) {
        let state = self.state;
        // SAFETY: this admitted free head is absent from the heap. Ordering
        // completes before its resident and FIFO membership are published below.
        unsafe { state.insert(self.index, priority) };
        // SAFETY: this capability retains the exact free head and the exclusive state loan.
        unsafe { state.free.acquire_at(&state.storage, self.index) };
        // SAFETY: the acquired slot is vacant with uninitialized value storage.
        unsafe { state.write(self.index, value) };
        // SAFETY: the same slot now holds a live value and its FIFO hook is detached.
        unsafe { state.ready.link_back(&state.storage, self.index) };
    }
}

const _: () =
    assert!(std::mem::size_of::<Vacant<'static, u64, u64>>() == 2 * std::mem::size_of::<usize>());

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

impl<P: Ord, V> Min<P, V> {
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

impl<P: Ord, V> Drop for Min<P, V> {
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

unsafe impl<P: Ord, V> heap::raw::Store for Min<P, V> {
    type Key = heap::Direct;
    type Priority = P;
    type Position = u32;
    type Storage = heap::Capacity<heap::Stored<heap::Direct, P>, u32, Node<V>>;
    type Extra = ptr::NonNull<Node<V>>;

    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &mut self.storage
    }
}

unsafe impl<P: Ord, V> crate::depot::raw::Hooks for Storage<P, V> {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        &unsafe { self.values().get_unchecked(index.raw() as usize) }.hook
    }
}

/// The single allocation binds heap priorities and resident payload nodes.
type Storage<P, V> = alloc::raw::Block<
    heap::Capacity<heap::Stored<heap::Direct, P>, u32, Node<V>>,
    u32,
    ptr::NonNull<Node<V>>,
>;
