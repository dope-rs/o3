use crate::{
    alloc,
    depot::{
        capacity,
        heap::{self, indexed},
    },
};

#[must_use = "an entry state must be committed to change the heap"]
pub enum Entry<'a, P: Ord, C: alloc::raw::Extent = capacity::Count> {
    Vacant(indexed::Vacant<'a, P, C>),
    Occupied(indexed::Occupied<'a, P, C>),
}

impl<'a, P: Ord, C: alloc::raw::Extent> Entry<'a, P, C> {
    pub(in crate::depot::heap::indexed) fn from_slot(
        slot: heap::Slot<'a, indexed::Min<P, C>>,
    ) -> Self {
        slot.map(
            (),
            |entry, ()| Self::Vacant(indexed::Vacant { entry }),
            |entry, ()| {
                Self::Occupied(indexed::Occupied {
                    entry: entry.into_parts().0,
                })
            },
        )
    }
}
