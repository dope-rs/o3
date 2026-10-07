use std::{marker, num::NonZero};

use heap::{Live, indexed};

use crate::{alloc, depot::heap};

/// One represented heap slot retaining its source context and phase.
pub struct Slot<'a, P: Ord, C: alloc::raw::Extent, T, M = ()> {
    owner: &'a mut indexed::Min<P, C>,
    context: T,
    position: NonZero<u32>,
    phase: marker::PhantomData<fn(M) -> M>,
}

impl<'a, P: Ord, C: alloc::raw::Extent, T, M> Slot<'a, P, C, T, M> {
    /// Consumes the exact initialized reverse-map position and its membership phase.
    pub fn from_source(live: Live<(&'a mut indexed::Min<P, C>, T), NonZero<u32>, M>) -> Self {
        let ((owner, context), position) = live.into_inner();
        Self {
            owner,
            context,
            position,
            phase: marker::PhantomData,
        }
    }
}

impl<'a, P: Ord, C: alloc::raw::Extent, T> Slot<'a, P, C, T> {
    pub fn entry(self) -> (indexed::Entry<'a, P, C>, T) {
        let index = self.position.get() as usize - 1;
        // SAFETY: this source retains the exact represented reverse-map position.
        let live = unsafe {
            crate::depot::raw::Proof::new((
                (self.owner, self.context),
                heap::Direct::from_index(index),
            ))
        };
        let (slot, context) = heap::Slot::from_source(live);
        (indexed::Entry::from_slot(slot), context)
    }
}

impl<'a, P: Ord, C: alloc::raw::Extent, T> Slot<'a, P, C, T, indexed::Empty> {
    pub fn entry(self) -> (indexed::Vacant<'a, P, C>, T) {
        // SAFETY: this vacant source retains one physical index below the owner's
        // fixed domain, so the direct key stays in range.
        let key = unsafe { heap::Direct::from_index(self.position.get() as usize - 1) };
        // SAFETY: the empty phase proves that index names a resident the owner
        // holds out of the heap, so its vacancy can be prepared.
        let entry = unsafe { heap::Vacancy::new(self.owner, key) };
        (indexed::Vacant { entry }, self.context)
    }
}

impl<'a, P: Ord, C: alloc::raw::Extent, T> Slot<'a, P, C, T, indexed::Full> {
    pub fn entry(self) -> (indexed::Occupied<'a, P, C>, T) {
        // SAFETY: this full source retains one physical index below the owner's
        // resident tail, which is initialized for every such index.
        let position = unsafe {
            *self
                .owner
                .storage
                .tail()
                .get_unchecked(self.position.get() as usize - 1)
        };
        // SAFETY: the full phase proves the represented reverse-map position is an
        // initialized heap position of this exact owner.
        let entry = unsafe { heap::Entry::new(self.owner, position as usize, ()) };
        (indexed::Occupied { entry }, self.context)
    }
}
