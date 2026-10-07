//! Fixed-domain sets with retained bounds and membership ownership.

use std::{array, cell, marker};

use bitmap::Error;
pub use entry::Entry;
pub use id::Id;

use crate::{
    alloc::{self, array::Block},
    depot::{
        index,
        index::{bitmap, bitmap::entry::Row, set},
    },
};

mod entry;
mod extent;
mod id;
mod storage;
mod views;
pub(in crate::depot::index::set) use extent::Extent;
pub(in crate::depot::index::set) use storage::Storage;
pub(in crate::depot::index::set) use views::NonEmpty;
pub(in crate::depot) use views::View;
pub mod view;

pub struct State<Tag = (), Raw = usize, const N: usize = 1, M = set::Planar> {
    block: Block<cell::Cell<usize>, set::Extent<Tag, Raw, N>>,
    mode: marker::PhantomData<fn(M) -> M>,
}

impl<Tag, Raw: crate::depot::raw::Word, const N: usize, M> State<Tag, Raw, N, M> {
    pub const fn capacity(&self) -> usize {
        self.block.extent().shape().capacity()
    }

    pub fn is_empty(&self, id: set::Id<N>) -> bool {
        bitmap::Access::is_empty(&set::View::new(self), id)
    }

    pub fn contains(&self, id: set::Id<N>, index: index::Ordinal<Tag, Raw>) -> bool {
        Row::try_new(set::View::new(self), index.offset()).is_ok_and(|row| row.contains(id))
    }
}

const _: () = assert!(std::mem::size_of::<State<(), u16, 5>>() == 2 * std::mem::size_of::<usize>());

/// Independently addressable membership planes.
pub struct Planar;

impl<Tag, Raw: crate::depot::raw::Word, const N: usize> set::State<Tag, Raw, N> {
    pub fn try_new(
        domain: index::Domain<Tag, Raw>,
        fill: [bitmap::Fill; N],
    ) -> Result<Self, alloc::Error> {
        let block = set::Extent::try_new(domain)?.try_allocate(fill)?;
        Ok(Self {
            block,
            mode: marker::PhantomData,
        })
    }

    /// Clears every plane and cursor in one pass over the packed storage.
    pub fn clear(&self) {
        for word in self.block.as_ref() {
            word.set(0);
        }
    }

    /// Checks the row once and retains this storage borrow for subsequent updates.
    pub fn try_entry(
        &self,
        index: index::Ordinal<Tag, Raw>,
    ) -> Result<set::Entry<&Self, Tag, Raw, N>, Error> {
        let row = Row::try_new(set::View::new(self), index.offset())?;
        Ok(set::Entry { row })
    }

    pub fn get<const P: usize>(&self) -> view::Plane<'_, Tag, Raw, N> {
        self.plane(set::Id::at::<P>())
    }

    pub fn plane(&self, id: set::Id<N>) -> view::Plane<'_, Tag, Raw, N> {
        view::Plane::new(self, id)
    }
}

/// Disjoint membership planes with affine removed memberships.
pub struct Disjoint;

impl<Tag, Raw: crate::depot::raw::Word, const N: usize> set::State<Tag, Raw, N, Disjoint> {
    pub fn try_disjoint(
        domain: index::Domain<Tag, Raw>,
        initial: set::Id<N>,
    ) -> Result<Self, alloc::Error> {
        let fill = array::from_fn(|id| {
            if id == initial.offset() {
                bitmap::Fill::Full
            } else {
                bitmap::Fill::Empty
            }
        });
        let block = set::Extent::try_new(domain)?.try_allocate(fill)?;
        Ok(Self {
            block,
            mode: marker::PhantomData,
        })
    }

    pub fn take(&self, id: set::Id<N>) -> Option<set::Entry<&Self, Tag, Raw, N, set::Id<N>>> {
        let start = set::NonEmpty::new(self)?.cursor(id).get();
        let removed = bitmap::pop_from(set::View::new(self), id, start)?;
        Some(set::Entry::new(removed))
    }

    pub fn take_from(
        &self,
        id: set::Id<N>,
        start: index::Ordinal<Tag, Raw>,
    ) -> Option<set::Entry<&Self, Tag, Raw, N, set::Id<N>>> {
        let removed = bitmap::Access::pop_from(set::View::new(self), id, start.offset())?;
        Some(set::Entry::new(removed))
    }
}

const _: () = assert!(
    std::mem::size_of::<set::State<(), u16, 2, Disjoint>>() == 2 * std::mem::size_of::<usize>()
);

// SAFETY: a shared borrow retains one immutable allocation and extent.
unsafe impl<Tag, Raw, const N: usize> crate::depot::raw::SetSource<Tag, Raw, N>
    for &set::State<Tag, Raw, N>
{
    fn planes(&self) -> &set::State<Tag, Raw, N> {
        self
    }
}

// SAFETY: an exclusive borrow retains the full owner for the selected operation.
unsafe impl<Tag, Raw, const N: usize> crate::depot::raw::SetSource<Tag, Raw, N>
    for &mut set::State<Tag, Raw, N>
{
    fn planes(&self) -> &set::State<Tag, Raw, N> {
        self
    }
}
