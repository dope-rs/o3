use std::marker;

use crate::depot::{
    queue::{indexed::slab, links},
    raw::Live,
};

/// An affine link retained inside its allocation-owning ledger.
#[repr(transparent)]
pub(in crate::depot::queue::group) struct Node<Tag>(Key<Tag>);

impl<Tag> Node<Tag> {
    pub(in crate::depot::queue::group) fn inserted(key: slab::Handle<Tag>) -> Self {
        Self((links::Index::from(key), marker::PhantomData))
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
type Key<Tag> = (links::Index, marker::PhantomData<slab::Handle<Tag>>);
