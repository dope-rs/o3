use crate::depot::{
    array::Index,
    queue::{links, tagged},
};

/// An entry and a destination in the same statically bounded owner.
pub struct Position<E: crate::depot::raw::TagEntry, const N: usize> {
    pub(in crate::depot::queue::tagged) entry: E,
    lane: Index<N>,
}

impl<E: crate::depot::raw::TagEntry, const N: usize> Position<E, N>
where
    E::Store: crate::depot::raw::TagStore<Heads = [links::Chain; N]>,
{
    pub(in crate::depot) fn new(entry: E, lane: Index<N>) -> Self {
        Self { entry, lane }
    }

    pub(in crate::depot) fn destination(&self) -> tagged::Endpoint<&E::Store> {
        tagged::Endpoint::indexed(self.entry.store(), self.lane)
    }
}

impl<E: crate::depot::raw::TagEntry, const N: usize> Position<E, N> {
    pub(in crate::depot) fn entry(&self) -> &E {
        &self.entry
    }
}
