use std::num::NonZero;

use heap::{Live, indexed};

use crate::{alloc, depot::heap};

impl<'a, P: Ord, C: alloc::raw::Extent, T> Live<(&'a mut indexed::Min<P, C>, T), NonZero<u32>> {
    pub fn entry(self) -> (indexed::Entry<'a, P, C>, T) {
        let ((owner, context), position) = self.into_inner();
        let index = position.get() as usize - 1;
        // SAFETY: this source retains the exact represented reverse-map position.
        let live = unsafe {
            crate::depot::raw::Proof::new(((owner, context), heap::Direct::from_index(index)))
        };
        let (slot, context) = heap::Slot::from_source(live);
        (indexed::Entry::from_slot(slot), context)
    }
}

impl<'a, P: Ord, C: alloc::raw::Extent, T>
    Live<(&'a mut indexed::Min<P, C>, T), NonZero<u32>, indexed::Empty>
{
    pub fn entry(self) -> (indexed::Vacant<'a, P, C>, T) {
        let ((owner, context), position) = self.into_inner();
        // SAFETY: this vacant source retains one physical index below the owner's
        // fixed domain, so the direct key stays in range.
        let key = unsafe { heap::Direct::from_index(position.get() as usize - 1) };
        // SAFETY: the empty phase proves that index names a resident the owner
        // holds out of the heap, so its vacancy can be prepared.
        let entry = unsafe { heap::Vacancy::new(owner, key) };
        (indexed::Vacant { entry }, context)
    }
}

impl<'a, P: Ord, C: alloc::raw::Extent, T>
    Live<(&'a mut indexed::Min<P, C>, T), NonZero<u32>, indexed::Full>
{
    pub fn entry(self) -> (indexed::Occupied<'a, P, C>, T) {
        let ((owner, context), position) = self.into_inner();
        // SAFETY: this full source retains one physical index below the owner's
        // resident tail, which is initialized for every such index.
        let position = unsafe {
            *owner
                .storage
                .tail()
                .get_unchecked(position.get() as usize - 1)
        };
        // SAFETY: the full phase proves the represented reverse-map position is an
        // initialized heap position of this exact owner.
        let entry = unsafe { heap::Entry::new(owner, position as usize, ()) };
        (indexed::Occupied { entry }, context)
    }
}
