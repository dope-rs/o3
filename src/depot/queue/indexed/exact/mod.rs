//! FIFO admission requiring one exact value per physical index.

use std::{mem, num, ops::ControlFlow};

pub use bounds::Bounds;

use crate::depot::{
    queue::{
        indexed::{exact, fifo},
        links,
    },
    raw::Live,
};

mod bounds;

/// Exact-value equality governs rescheduling within each physical slot.
#[derive(Debug)]
pub enum Policy {}

impl<T: Copy + Eq, I: crate::depot::raw::Compact> fifo::List<T, I, exact::Policy> {
    /// Admits a bounded pass before acquiring its context; every visit retains its exact slot.
    pub fn try_sweep<C, E, B>(
        &mut self,
        limit: usize,
        acquire: impl FnOnce(num::NonZero<usize>) -> Result<C, E>,
        mut visit: impl FnMut(C, exact::Entry<'_, T, I>, T) -> ControlFlow<B, C>,
    ) -> Result<Option<ControlFlow<B, C>>, E> {
        let Some(count) = num::NonZero::new(limit.min(self.len())) else {
            return Ok(None);
        };
        Ok(Some((0..count.get()).try_fold(
            acquire(count)?,
            |context, _| {
                // SAFETY: admission retains every unvisited head; each earlier loan
                // can mutate only its own popped slot.
                let source = unsafe { crate::depot::raw::Proof::new(&self.store) };
                let (entry, value) = fifo::Store::front_from(source).take();
                // SAFETY: this physical identity belongs to the retained compact domain.
                let index = unsafe { I::from_raw(entry.index().raw()) };
                visit(context, exact::Entry { owner: self, index }, value)
            },
        )))
    }

    pub fn try_entry(&mut self, index: I) -> Result<exact::Entry<'_, T, I>, exact::Bounds<I>> {
        let capacity = self.store.capacity();
        if index.into_raw() as usize >= capacity {
            return Err(exact::Bounds { index, capacity });
        }
        Ok(exact::Entry { owner: self, index })
    }

    pub fn front(&self) -> Option<T> {
        self.store.front_entry().map(|occupied| *occupied.get())
    }

    /// Removes the head while retaining its original physical-slot admission.
    pub fn pop(&mut self) -> Option<(Live<&mut Self, I, ()>, T)> {
        let (entry, value) = self.store.front_entry()?.take();
        let index = entry.index();
        // SAFETY: the retained owner still contains the exact initialized physical node.
        Some((
            unsafe { crate::depot::raw::Proof::new((self, I::from_raw(index.raw()))) },
            value,
        ))
    }
}

/// One admitted identity retaining exclusive access to its original FIFO.
#[must_use]
pub struct Entry<'a, T: Copy + Eq, I: crate::depot::raw::Compact = u32> {
    pub(in crate::depot::queue::indexed::exact) owner: &'a mut fifo::List<T, I, exact::Policy>,
    pub(in crate::depot::queue::indexed::exact) index: I,
}

impl<'a, T: Copy + Eq, I: crate::depot::raw::Compact> Entry<'a, T, I> {
    pub fn index(&self) -> I {
        self.index
    }

    /// Projects an admitted node with the complete joint source still borrowed.
    pub fn from_source<C>(source: Admission<'a, T, I, C>) -> (Self, C) {
        let ((owner, context), index) = source.into_inner();
        (Self { owner, index }, context)
    }

    pub fn schedule(&mut self, value: T) {
        self.slot().replace::<true>(value);
    }

    fn slot(&mut self) -> fifo::NodeEntry<'_, T, links::Chain<u32>> {
        let store = &self.owner.store.nodes;
        // SAFETY: admission retained this exact owner and represented identity.
        let index = unsafe { links::Index::new_usize(self.index.into_raw() as usize) };
        unsafe { store.at(index) }
    }

    pub fn unlink(&mut self, expected: T) {
        self.slot().unlink(expected);
    }

    /// Places this admitted node at the back, including an already queued value.
    pub fn requeue(&mut self, value: T) {
        self.slot().replace::<false>(value);
    }
}

// SAFETY: the incoming Live owns the admitted physical node and exclusive FIFO borrow.

const _: () = {
    assert!(mem::size_of::<Entry<'static, u64>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Entry<'static, u64>>>() == 2 * mem::size_of::<usize>());
};

/// The source context and FIFO loan retain one admitted identity.
type Admission<'a, T, I, C> = Live<(&'a mut fifo::List<T, I, exact::Policy>, C), I>;
