use std::{mem, ptr, slice};

use crate::depot::{self, queue::ring};

impl<T> ring::Fifo<T> {
    pub fn front(&self) -> Option<&T> {
        if self.state.is_empty() {
            return None;
        }
        // SAFETY: this nonempty FIFO's shared borrow retains its initialized head.
        Some(unsafe { &*self.block.as_ptr().add(self.state.head()).cast::<T>() })
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.state.len() {
            return None;
        }
        let position = unsafe { self.state.position(self.capacity(), index) };
        // SAFETY: the bounds check admits this initialized logical offset.
        Some(unsafe { &*self.block.as_ptr().add(position).cast::<T>() })
    }

    pub fn as_slices(&self) -> (&[T], &[T]) {
        let state = self.state;
        let (start, [front, back]) = state.prefix(self.capacity(), state.len());
        let entries = self.block.as_ptr().cast::<T>();
        // SAFETY: this FIFO retains the two exact initialized runs for this borrow.
        unsafe {
            (
                slice::from_raw_parts(entries.add(start), front),
                slice::from_raw_parts(entries, back),
            )
        }
    }
}

impl<T> ring::Cell<T> {
    /// Drops every resident value in place.
    pub fn clear(&self) {
        loop {
            let mut state = self.state.get();
            if state.is_empty() {
                return;
            }
            let index = unsafe { state.pop_front(self.capacity()) };
            self.state.set(state);
            // SAFETY: publish removal before Drop can reenter, then reload state.
            // Insertion cannot reuse the retiring slot until its destructor finishes.
            unsafe {
                self.block
                    .as_ptr()
                    .cast_mut()
                    .add(index)
                    .cast::<T>()
                    .drop_in_place()
            };
        }
    }
}

/// # Safety
/// Consume both initialized runs exclusively, without outstanding loans or reentry.
/// Do not access or destroy these elements again; clear their state before
/// any later owner operation, or finish destruction without revisiting them.
pub(crate) unsafe fn drop_ring<S: depot::raw::RingEdit + ?Sized>(source: &mut S) {
    if mem::needs_drop::<S::Item>() {
        // SAFETY: exclusive ownership retains both disjoint initialized runs.
        let (front, back) = unsafe { depot::raw::RingEdit::slices_mut(source) };
        // SAFETY: both runs are consumed exactly once without competing access.
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
