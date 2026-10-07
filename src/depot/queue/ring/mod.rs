//! Exact-capacity local ring queues.

use std::{cell, iter, marker, mem, num::NonZero, slice};

pub use full::Full;
pub use lease::Lease;
pub use pool::Pool;

use crate::{
    alloc::array,
    depot::{capacity, queue::ring},
};

mod full;
pub mod inline;

mod lease;
mod pool;
mod state;
pub use state::State;

/// An exclusive FIFO with one exact, fixed backing allocation.
pub struct Fifo<T> {
    pub(in crate::depot) block: array::Block<mem::MaybeUninit<T>, capacity::Count>,
    pub(in crate::depot) state: ring::State<u32>,
}

/// The initialized front tied to an exclusive queue borrow.
#[must_use]
pub struct Front<'a, S: ?Sized> {
    pub(in crate::depot::queue::ring) source: &'a mut S,
}

/// One queue vacancy tied to its exclusive owner borrow.
#[must_use]
pub struct Vacant<'a, S: ?Sized> {
    pub(in crate::depot::queue::ring) source: &'a mut S,
}

/// A thread-confined exact FIFO whose operations use a shared borrow.
pub struct Cell<T> {
    pub(in crate::depot) block: array::Block<mem::MaybeUninit<T>, capacity::Count>,
    pub(in crate::depot) state: cell::Cell<ring::State<u32>>,
    invariant: marker::PhantomData<cell::Cell<T>>,
}

impl<T> Fifo<T> {
    /// Allocates exactly `capacity` entries.
    pub fn try_new(capacity: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            block: array::Block::try_new(capacity, ())?,
            state: ring::State::EMPTY,
        })
    }

    /// Allocates a nonempty domain and queues the optional first value.
    pub fn try_with(capacity: NonZero<u32>, first: Option<T>) -> Result<Self, crate::alloc::Error> {
        let mut queue = Self::try_new(capacity::Count::new(capacity.get()))?;
        if let Some(first) = first {
            // SAFETY: the retained nonzero capacity admits this first value.
            unsafe { crate::depot::raw::RingEdit::vacant_raw(&mut queue) }.push_back(first);
        }
        Ok(queue)
    }

    pub fn vacant(&mut self) -> Option<Vacant<'_, Fifo<T>>> {
        self.try_vacant().ok()
    }

    pub fn try_vacant(&mut self) -> Result<Vacant<'_, Fifo<T>>, ring::Full> {
        let used = self.state.len();
        let capacity = self.capacity();
        if used == capacity {
            return Err(ring::Full { used, capacity });
        }
        Ok(ring::Vacant { source: self })
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        let Some(vacant) = self.vacant() else {
            return Err(value);
        };
        vacant.push_back(value);
        Ok(())
    }

    pub fn pop_front(&mut self) -> Option<T> {
        Some(self.front_entry()?.remove())
    }

    pub fn front(&self) -> Option<&T> {
        crate::depot::raw::RingRead::front(self)
    }

    /// Borrows the current front exclusively so inspection and removal refer
    /// to the same entry without a second occupancy check.
    pub fn front_entry(&mut self) -> Option<Front<'_, Fifo<T>>> {
        if self.state.is_empty() {
            return None;
        }
        Some(ring::Front { source: self })
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        crate::depot::raw::RingRead::get(self, index)
    }

    pub fn as_slices(&self) -> (&[T], &[T]) {
        crate::depot::raw::RingRead::as_slices(self)
    }

    pub fn clear(&mut self) {
        if !mem::needs_drop::<T>() && !self.state.is_empty() {
            // SAFETY: this nonempty queue retains a valid head, length, and capacity.
            unsafe { self.state.clear(self.capacity()) };
            return;
        }
        while let Some(front) = self.front_entry() {
            front.discard();
        }
    }

    pub const fn capacity(&self) -> usize {
        self.block.extent().get()
    }

    pub fn len(&self) -> usize {
        self.state.len()
    }

    pub fn count(&self) -> capacity::Count {
        self.state.count()
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.state.is_full(self.capacity())
    }
}

impl<'a, S: crate::depot::raw::RingEdit + ?Sized> Front<'a, S> {
    /// # Safety
    /// This exact exclusive source retains an initialized front.
    pub(in crate::depot) unsafe fn new(source: &'a mut S) -> Self {
        Self { source }
    }

    pub fn get(&self) -> &S::Item {
        // SAFETY: Front retains the exact initialized head and original owner.
        unsafe {
            &*self
                .source
                .entries()
                .add(self.source.state().head())
                .cast::<S::Item>()
        }
    }

    pub fn get_mut(&mut self) -> &mut S::Item {
        // SAFETY: the exclusive Front borrow protects this initialized payload.
        unsafe {
            let (state, entries) = self.source.parts_mut();
            &mut *entries.as_mut_ptr().add(state.head()).cast::<S::Item>()
        }
    }

    pub fn remove(self) -> S::Item {
        self.detach(|entry| unsafe { entry.read() })
    }

    pub fn take(self) -> (S::Item, ring::Vacant<'a, S>) {
        let source = self.source;
        let value = ring::Front { source }.remove();
        (value, ring::Vacant { source })
    }

    /// Removes and destroys this initialized front in place.
    pub fn discard(self) {
        // SAFETY: detach publishes removal before running the resident destructor.
        unsafe { self.detach(|entry| entry).drop_in_place() };
    }

    fn detach<R>(self, read: impl FnOnce(*mut S::Item) -> R) -> R {
        // SAFETY: Front carries nonempty admission under the exclusive owner.
        unsafe {
            let (state, entries) = self.source.parts_mut();
            let value = read(entries.as_mut_ptr().add(state.head()).cast::<S::Item>());
            state.pop_front(entries.len());
            value
        }
    }
}

impl<'a, S: crate::depot::raw::RingEdit + ?Sized> Vacant<'a, S> {
    pub(in crate::depot) fn into_source(self) -> &'a mut S {
        self.source
    }

    /// # Safety
    /// This exact exclusive source retains one vacant slot.
    pub(in crate::depot) unsafe fn new(source: &'a mut S) -> Self {
        Self { source }
    }

    pub fn push_back(self, value: S::Item) {
        crate::depot::raw::RingEdit::push::<false>(self, value);
    }

    pub fn push_front(self, value: S::Item) {
        crate::depot::raw::RingEdit::push::<true>(self, value);
    }
}

impl<T> Cell<T> {
    /// Allocates exactly `capacity` entries.
    pub fn try_new(capacity: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            block: array::Block::try_new(capacity, ())?,
            state: cell::Cell::new(ring::State::EMPTY),
            invariant: marker::PhantomData,
        })
    }

    pub fn pop_front(&self) -> Option<T> {
        crate::depot::raw::RingCell::take_front(self)
    }

    /// Drops every resident value in place.
    pub fn clear(&self) {
        crate::depot::raw::RingDestroy::clear(self);
    }

    pub const fn capacity(&self) -> usize {
        self.block.extent().get()
    }

    pub fn is_empty(&self) -> bool {
        self.state.get().is_empty()
    }
}

impl<T> Drop for Fifo<T> {
    fn drop(&mut self) {
        // SAFETY: Drop is the terminal exclusive owner of both resident runs.
        unsafe { crate::depot::raw::drop_ring(self) };
    }
}

impl<T> Drop for Cell<T> {
    fn drop(&mut self) {
        // SAFETY: Drop is the terminal exclusive owner of both resident runs.
        unsafe { crate::depot::raw::drop_ring(self) };
    }
}

const _: () = {
    assert!(mem::size_of::<Fifo<u64>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Cell<u64>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Front<'static, Fifo<u64>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Front<'static, Fifo<u64>>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Vacant<'static, Fifo<u64>>>() == mem::size_of::<usize>());
};

impl<'a, T> IntoIterator for &'a Fifo<T> {
    type Item = &'a T;
    type IntoIter = iter::Chain<slice::Iter<'a, T>, slice::Iter<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        let (front, back) = self.as_slices();
        front.iter().chain(back)
    }
}

impl<'a, T> IntoIterator for &'a mut Fifo<T> {
    type Item = &'a mut T;
    type IntoIter = iter::Chain<slice::IterMut<'a, T>, slice::IterMut<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        // SAFETY: changing FIFO values preserves its state and capacity.
        let (front, back) = unsafe { crate::depot::raw::RingEdit::slices_mut(self) };
        front.iter_mut().chain(back)
    }
}
