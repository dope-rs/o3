use std::num::NonZero;

use crate::depot::{
    self,
    queue::{links, tagged},
};

/// # Safety
/// Default is empty; every projection retains this lane's original first/last cells.
/// TAIL equals tail presence; absent tails reject back adjacency; mutations preserve reciprocal links.
pub unsafe trait Head: Default {
    const TAIL: bool;

    fn first(&self) -> Option<links::Index>;
    fn adjacent<const FRONT: bool>(&self) -> Option<links::Index>;
    /// Returns this lane's original Chain when it owns a tail.
    fn tail(&self) -> Option<&links::Chain>;

    /// # Safety
    /// The caller preserves this lane's reciprocal first-node relation.
    unsafe fn set_first(&self, index: Option<links::Index>);
}

// SAFETY: Chain retains its original first and last cells and defaults to empty.
unsafe impl Head for links::Chain {
    const TAIL: bool = true;

    fn first(&self) -> Option<links::Index> {
        self.first()
    }

    fn adjacent<const FRONT: bool>(&self) -> Option<links::Index> {
        if FRONT { self.first() } else { self.last() }
    }

    fn tail(&self) -> Option<&links::Chain> {
        Some(self)
    }

    unsafe fn set_first(&self, index: Option<links::Index>) {
        self.set_first(index);
    }
}

// SAFETY: List owns only its first cell; it never admits back insertion.
unsafe impl Head for links::List {
    const TAIL: bool = false;

    fn first(&self) -> Option<links::Index> {
        self.first()
    }

    fn adjacent<const FRONT: bool>(&self) -> Option<links::Index> {
        const { assert!(FRONT) };
        self.first()
    }

    fn tail(&self) -> Option<&links::Chain> {
        None
    }

    unsafe fn set_first(&self, index: Option<links::Index>) {
        self.set(index);
    }
}

// SAFETY: the proof retains this owner's exact represented head below the sentinel.
unsafe impl<'a, S: depot::raw::TagStore> depot::raw::Project<usize, tagged::Endpoint<&'a S>>
    for &'a S
{
    type Output = tagged::Endpoint<Self>;

    unsafe fn project(self, index: usize) -> Self::Output {
        unsafe { tagged::Endpoint::new(self, NonZero::new_unchecked(u32::MAX - index as u32)) }
    }
}
