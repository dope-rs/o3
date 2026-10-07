//! Allocation-neutral lifecycle for lazily initialized linked slots.

use std::{cell, marker, mem, num::NonZero, ptr};

pub(crate) use input::Input;

use crate::{
    alloc,
    base::num::bounded,
    depot::{
        arena::linked::{self, fifo, slots},
        capacity,
    },
};

mod input;

/// Access to one exact linked-slot region without owning its allocation.
#[derive(Debug)]
pub struct Store<T> {
    pub(crate) region: Region<T>,
    pub(super) free: cell::Cell<u32>,
    pub(super) available: cell::Cell<u32>,
    ownership: marker::PhantomData<T>,
}

/// Moving the descriptor preserves its region pointer and resident addresses.
impl<T> Unpin for Store<T> {}

/// Address and initialized prefix of one exact slot allocation.
#[derive(Debug)]
pub(crate) struct Region<T> {
    slots: ptr::NonNull<mem::MaybeUninit<slots::Node<T>>>,
    capacity: slots::Capacity<T>,
    initialized: cell::Cell<u32>,
}

impl<T> Region<T> {
    const unsafe fn new(base: ptr::NonNull<slots::Node<T>>, capacity: slots::Capacity<T>) -> Self {
        Self {
            slots: base.cast(),
            capacity,
            initialized: cell::Cell::new(0),
        }
    }

    pub(crate) const fn capacity(&self) -> slots::Capacity<T> {
        self.capacity
    }

    pub(crate) const fn pointer(&self) -> ptr::NonNull<u8> {
        self.slots.cast()
    }

    /// # Safety
    /// The initialized prefix has unused capacity below the reserved sentinel.
    unsafe fn init<const LIVE: bool>(&self) -> (u32, &slots::Node<T>) {
        let index = self.initialized.get();
        let node = unsafe { self.slots.as_ptr().add(index as usize) };
        let next = if LIVE { Store::<T>::END } else { index };
        unsafe {
            node.write(mem::MaybeUninit::new(slots::Node::vacant(next)));
        }
        self.initialized.set(index + 1);
        (index, unsafe { (&*node).assume_init_ref() })
    }

    /// # Safety
    /// `index` is below this region's initialized prefix.
    pub(super) unsafe fn slot(&self, index: u32) -> &slots::Node<T> {
        unsafe { (&*self.slots.as_ptr().add(index as usize)).assume_init_ref() }
    }
}

impl<T> Store<T> {
    pub(crate) const END: u32 = u32::MAX;

    pub(crate) const unsafe fn new(
        base: ptr::NonNull<slots::Node<T>>,
        capacity: slots::Capacity<T>,
    ) -> Self {
        Self {
            region: unsafe { Region::new(base, capacity) },
            free: cell::Cell::new(Self::END),
            available: cell::Cell::new(capacity.get()),
            ownership: marker::PhantomData,
        }
    }

    pub(crate) fn available(&self) -> usize {
        self.available.get() as usize
    }

    /// Returns this store's already-debited slot credits.
    pub(super) fn refund(&self, count: u32) {
        self.available.set(self.available.get().wrapping_add(count));
    }

    pub(in crate::depot::arena::linked) fn try_reserve(
        &self,
        count: capacity::Count,
    ) -> Result<(), capacity::Shortage> {
        let room = self.available.get();
        if count.raw() > room {
            return Err(capacity::Shortage {
                // SAFETY: demand exceeding the available count is nonzero.
                requested: unsafe { NonZero::new_unchecked(count.get()) },
                available: room as usize,
            });
        }
        self.available.set(room.wrapping_sub(count.raw()));
        Ok(())
    }

    /// # Safety
    /// One free slot is proved; LIVE requires immediate initialization before publication.
    pub(super) unsafe fn claim<const LIVE: bool>(&self) -> (u32, &slots::Node<T>) {
        let mut free = Free::new(self.free.get());
        let index = free.head;
        if index == Self::END {
            return unsafe { self.region.init::<LIVE>() };
        }
        let node = unsafe { self.region.slot(index) };
        let next = node.next.replace(if LIVE { Self::END } else { index });
        free.pop(index, next, Self::END);
        self.free.set(free.head);
        (index, node)
    }

    pub(super) fn recycle(&self, index: u32, node: &slots::Node<T>) {
        let mut free = Free::new(self.free.get());
        node.next.set(free.push(index, Self::END));
        self.free.set(free.head);
    }

    /// # Safety
    /// The caller solely owns this detached live slot with this exact TRACK policy.
    pub(crate) unsafe fn take<Tag, const TRACK: bool>(&self, index: slots::Index<Tag>) -> T {
        let node = unsafe { self.region.slot(index.raw()) };
        let value = unsafe { (&*node.value.get()).assume_init_read() };
        self.recycle(index.raw(), node);
        if TRACK {
            self.refund(1);
        }
        value
    }

    pub(crate) fn vacant<Tag>(&self) -> Option<slots::Vacant<T, &'_ slots::Store<T>, Tag>> {
        if self.available.get() == 0 {
            return None;
        }
        Some(unsafe { slots::Vacant::reserve(self) })
    }

    /// Returns one live slot's successor.
    /// # Safety
    /// `index` belongs to the calling topology in this region.
    pub(crate) unsafe fn next<Tag>(&self, index: slots::Index<Tag>) -> slots::Index<Tag> {
        slots::Index::new(unsafe { self.region.slot(index.raw()) }.next.get())
    }

    /// Replaces one live slot's successor.
    /// # Safety
    /// `index` is live and `next` is empty or live in the same topology domain.
    pub(crate) unsafe fn set_next<Tag>(&self, index: slots::Index<Tag>, next: slots::Index<Tag>) {
        unsafe { self.region.slot(index.raw()) }
            .next
            .set(next.raw());
    }

    /// Borrows one live value.
    /// # Safety
    /// The owner borrow prevents mutation or release of `index`.
    pub(crate) unsafe fn value<Tag>(&self, index: slots::Index<Tag>) -> &T {
        unsafe { &*self.value_ptr(index) }
    }

    /// Projects the address of one live value without creating a reference.
    /// # Safety
    /// `index` is live; reference aliases must derive from its exact owner.
    pub(crate) unsafe fn value_ptr<Tag>(&self, index: slots::Index<Tag>) -> *mut T {
        unsafe { self.region.slot(index.raw()) }.value.get().cast()
    }

    /// Binds the sole detached live slot ownership to this store.
    /// # Safety
    /// `index` is absent from every owner topology and outstanding capability.
    pub(crate) unsafe fn detached<Tag>(
        &self,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, &'_ slots::Store<T>, Tag> {
        unsafe { slots::Live::detached(self, index) }
    }

    pub(crate) unsafe fn drop_values(&mut self) {
        if !mem::needs_drop::<T>() {
            return;
        }
        let mut free = self.free.get();
        while free != Self::END {
            let slot = unsafe { self.region.slot(free) };
            let next = slot.next.replace(free);
            free = if next == free { Self::END } else { next };
        }
        for offset in 0..self.region.initialized.get() {
            let slot = unsafe {
                (&mut *self.region.slots.as_ptr().add(offset as usize)).assume_init_mut()
            };
            if slot.next.get() != offset {
                unsafe { (&mut *slot.value.get()).assume_init_drop() };
            }
        }
    }
}

impl<T> linked::Sealed<T> for Store<T> {}

impl<T> linked::Source<T> for Store<T> {
    fn store(&self) -> &Store<T> {
        self
    }
}

const _: () = {
    assert!(mem::size_of::<Region<u64>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Store<u64>>() == 3 * mem::size_of::<usize>());
};

#[repr(C)]
pub(crate) struct Node<T> {
    pub(super) value: cell::UnsafeCell<mem::MaybeUninit<T>>,
    pub(super) next: cell::Cell<u32>,
}

impl<T> Node<T> {
    pub(crate) const fn vacant(index: u32) -> Self {
        Self {
            value: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
            next: cell::Cell::new(index),
        }
    }

    /// # Safety
    /// The caller solely owns this detached initialized node at `index`.
    pub(super) unsafe fn clear(&self, index: u32) {
        // SAFETY: the caller consumes this node's initialized value once.
        unsafe { (&mut *self.value.get()).assume_init_drop() };
        self.next.set(index);
    }
}

/// A slot count whose exact allocation layout has been validated.
#[repr(transparent)]
#[derive(Debug)]
pub(crate) struct Capacity<T> {
    /// The absent-link sentinel is excluded from every admitted extent.
    raw: bounded::U32<0, { u32::MAX - 1 }>,
    value: marker::PhantomData<fn(T) -> T>,
}

impl<T> Capacity<T> {
    pub(crate) fn new<N: capacity::Extent>(capacity: N) -> Result<Self, alloc::Overflow> {
        let raw =
            bounded::U32::from_usize(capacity::Extent::get(capacity)).ok_or(alloc::Overflow)?;
        alloc::raw::Layout::<Node<T>>::try_check(raw.into_usize())?;
        Ok(Self {
            raw,
            value: marker::PhantomData,
        })
    }

    pub(crate) const fn get(self) -> u32 {
        self.raw.get()
    }

    pub(crate) const fn len(self) -> usize {
        self.raw.get() as usize
    }

    pub(crate) fn bundle(self) -> alloc::raw::Bundle<alloc::raw::Layout<Node<T>>> {
        use alloc::raw::Bundle;

        unsafe { Bundle::root::<Node<T>>(self.len()) }
    }
}

impl<T> Clone for Capacity<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Capacity<T> {}

const _: () = assert!(mem::size_of::<Capacity<u64>>() == mem::size_of::<u32>());
const _: () = assert!(mem::size_of::<Node<u64>>() == 2 * mem::size_of::<u64>());

impl<T> Store<T> {
    pub(in crate::depot) fn permit<Tag>(&mut self) -> Option<slots::Permit<'_, T, Tag>> {
        if self.available() == 0 {
            return None;
        }
        Some(slots::Permit::new(self))
    }
}

impl<T> Store<T> {
    /// # Safety
    /// `back` remains live in this exact exclusively borrowed node set.
    pub(in crate::depot) unsafe fn back_mut<Tag>(&mut self, back: fifo::Back<Tag>) -> &mut T {
        unsafe { &mut *self.value_ptr(back.into_index()) }
    }
}

/// The shared self-terminated encoding of an embedded free chain.
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Free<I> {
    pub(super) head: I,
}

impl<I: Copy + Eq> Free<I> {
    pub(super) const fn new(head: I) -> Self {
        Self { head }
    }

    pub(super) fn pop(&mut self, index: I, next: I, empty: I) {
        self.head = if next == index { empty } else { next };
    }

    pub(super) fn push(&mut self, index: I, empty: I) -> I {
        let next = if self.head == empty { index } else { self.head };
        self.head = index;
        next
    }
}
