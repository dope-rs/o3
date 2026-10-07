use std::{mem, ptr, slice};

use crate::depot::{self, queue::ring};

/// A ring whose shared borrow keeps every resident value stable.
/// # Safety
/// No operation through a shared borrow may move, replace, or destroy a value.
pub(crate) unsafe trait RingRead: depot::raw::RingStore {
    fn front(&self) -> Option<&Self::Item> {
        if self.state().is_empty() {
            return None;
        }
        Some(unsafe { &*self.entries().add(self.state().head()).cast::<Self::Item>() })
    }

    fn get(&self, index: usize) -> Option<&Self::Item> {
        if index >= self.state().len() {
            return None;
        }
        let position = unsafe { self.state().position(self.capacity(), index) };
        // SAFETY: the bounds check admits this initialized logical offset.
        Some(unsafe { &*self.entries().add(position).cast::<Self::Item>() })
    }

    fn as_slices(&self) -> (&[Self::Item], &[Self::Item]) {
        let state = self.state();
        let (start, [front, back]) = state.prefix(self.capacity(), state.len());
        let entries = self.entries().cast::<Self::Item>();
        // SAFETY: RingRead retains the two exact initialized runs for this borrow.
        unsafe {
            (
                slice::from_raw_parts(entries.add(start), front),
                slice::from_raw_parts(entries, back),
            )
        }
    }
}

/// # Safety
/// A retiring slot cannot be reused until its in-place destructor finishes.
pub(crate) unsafe trait RingDestroy: depot::raw::RingCell {
    fn clear(&self) {
        loop {
            let mut state = self.state();
            if state.is_empty() {
                return;
            }
            let index = unsafe { state.pop_front(self.capacity()) };
            unsafe { self.publish(state) };
            // SAFETY: publish removal before Drop can reenter, then reload state.
            unsafe {
                self.entries()
                    .cast_mut()
                    .add(index)
                    .cast::<Self::Item>()
                    .drop_in_place()
            };
        }
    }
}

/// # Safety
/// The owner is being destroyed and will never access its values again.
pub(crate) unsafe fn drop_ring<S: depot::raw::RingEdit + ?Sized>(source: &mut S) {
    if mem::needs_drop::<S::Item>() {
        // SAFETY: terminal ownership excludes every subsequent owner access.
        let (front, back) = unsafe { depot::raw::RingEdit::slices_mut(source) };
        // SAFETY: terminal ownership consumes both initialized runs exactly once.
        unsafe {
            ptr::drop_in_place(front);
            ptr::drop_in_place(back);
        }
    }
}

// SAFETY: state and backing are disjoint parts of this exact exclusive owner.
unsafe impl<T> crate::depot::raw::RingEdit for ring::Fifo<T> {
    unsafe fn parts_mut(&mut self) -> (&mut ring::State<u32>, &mut [mem::MaybeUninit<T>]) {
        (&mut self.state, &mut self.block)
    }
}

// SAFETY: operations publish synchronously and never lend resident references.
unsafe impl<T> crate::depot::raw::RingCell for ring::Cell<T> {
    unsafe fn publish(&self, state: ring::State<u32>) {
        self.state.set(state);
    }
}

// SAFETY: exclusive ownership rules out shared mutations of this disjoint state and backing.
unsafe impl<T> crate::depot::raw::RingEdit for ring::Cell<T> {
    unsafe fn parts_mut(&mut self) -> (&mut ring::State<u32>, &mut [mem::MaybeUninit<T>]) {
        (self.state.get_mut(), &mut self.block)
    }
}

// SAFETY: exclusive ownership rules out all outstanding leases and shared mutations.
unsafe impl<T> crate::depot::raw::RingEdit for ring::Pool<T> {
    unsafe fn parts_mut(&mut self) -> (&mut ring::State<u32>, &mut [mem::MaybeUninit<T>]) {
        (self.state.get_mut(), &mut self.block)
    }
}

// SAFETY: affine reservations protect vacant slots; shared operations lend no values.
unsafe impl<T> crate::depot::raw::RingCell for ring::Pool<T> {
    unsafe fn publish(&self, state: ring::State<u32>) {
        self.state.set(state);
    }
}

// SAFETY: this owner initializes exactly its state's logical range in block.
unsafe impl<T> crate::depot::raw::RingStore for ring::Fifo<T> {
    type Item = T;
    type Word = u32;

    fn state(&self) -> ring::State<u32> {
        self.state
    }
    fn capacity(&self) -> usize {
        self.capacity()
    }
    fn entries(&self) -> *const mem::MaybeUninit<T> {
        self.block.as_ptr()
    }
}

// SAFETY: every resident mutation requires an exclusive ring::Fifo borrow.
unsafe impl<T> crate::depot::raw::RingRead for ring::Fifo<T> {}

// SAFETY: the shared cell owns exactly the initialized range described by state.
unsafe impl<T> crate::depot::raw::RingStore for ring::Cell<T> {
    type Item = T;
    type Word = u32;
    fn state(&self) -> ring::State<u32> {
        self.state.get()
    }
    fn capacity(&self) -> usize {
        self.capacity()
    }
    fn entries(&self) -> *const mem::MaybeUninit<T> {
        self.block.as_ptr()
    }
}

// SAFETY: ring::Cell insertion requires a non-reentrant unsafe admission.
unsafe impl<T> crate::depot::raw::RingDestroy for ring::Cell<T> {}

// SAFETY: ring::Pool retains the exact backing and initialized range across shared operations.
unsafe impl<T> crate::depot::raw::RingStore for ring::Pool<T> {
    type Item = T;
    type Word = u32;

    fn state(&self) -> ring::State<u32> {
        self.state.get()
    }
    fn capacity(&self) -> usize {
        self.capacity()
    }
    fn entries(&self) -> *const mem::MaybeUninit<T> {
        self.block.as_ptr()
    }
}
