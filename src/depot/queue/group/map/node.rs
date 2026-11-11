use std::marker;

use crate::depot::{
    queue::{group::map, indexed::slab, links},
    raw::Live,
};

/// An affine link retained inside its allocation-owning ledger.
#[repr(transparent)]
pub(in crate::depot::queue::group) struct Node<Tag>(Key<Tag>);

impl<Tag> Node<Tag> {
    pub(in crate::depot::queue::group) fn new(index: links::Index) -> Self {
        Self((index, marker::PhantomData))
    }

    pub(in crate::depot::queue::group) fn index(&self) -> links::Index {
        self.0.0
    }

    pub(in crate::depot::queue::group) fn inserted(key: slab::Handle<Tag>) -> Self {
        Self::new(links::Index::from(key))
    }

    pub(in crate::depot::queue::group) fn bind<'a, S>(&'a self, source: S) -> Live<S, &'a Key<Tag>>
    where
        S: crate::depot::raw::Project<&'a Key<Tag>>,
    {
        // SAFETY: only the ledger owning both this affine link and its allocation
        // can borrow Node. Unlinking consumes the link before retiring its slot.
        unsafe { crate::depot::raw::Proof::new((source, &self.0)) }
    }
}

const _: () = {
    assert!(size_of::<Node<()>>() == size_of::<links::Index>());
    assert!(size_of::<Option<Node<()>>>() == size_of::<links::Index>());
};

/// A compact index retaining its slab's invariant tag without a stored generation.
pub(super) type Key<Tag> = (links::Index, marker::PhantomData<slab::Handle<Tag>>);

impl<'id, const LANES: usize> map::Draft<'_, 'id, LANES> {
    pub(in crate::depot::queue::group) fn member(
        &self,
        key: map::draft::Key<'id>,
    ) -> Node<map::Tag> {
        Node(key.into_inner())
    }
}
