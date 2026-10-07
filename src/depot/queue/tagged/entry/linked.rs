use std::{marker, num::NonZero};

use crate::depot::{
    index::Initialized,
    queue::{indexed::lanes, links, tagged},
};

/// Exclusive live membership, with the original initialized entry intact.
pub struct Linked<E: crate::depot::raw::TagEntry, P: crate::depot::raw::TagFinish<E> = ()> {
    entry: P::Storage,
    policy: marker::PhantomData<P>,
}

impl<E: crate::depot::raw::TagEntry> Linked<E> {
    /// # Safety
    /// The entry was obtained from its live lane or just inserted by Vacant.
    pub(in crate::depot) unsafe fn new(entry: E) -> Self {
        Self {
            entry,
            policy: marker::PhantomData,
        }
    }
}

impl<'a, S: crate::depot::raw::LaneStore>
    Linked<crate::depot::raw::Proof<(&'a mut S, links::Index), Initialized<links::Chain>>>
{
    pub(in crate::depot) fn at(store: &'a mut S, index: usize) -> Option<Self> {
        let node = store.nodes().get(index)?;
        // SAFETY: this exact owner's immutable node extent admits the index.
        let index = unsafe { links::Index::new_usize(index) };
        if !node.hook.is_linked(index) {
            return None;
        }
        // SAFETY: the checked live hook retains its initialized payload under
        // the original exclusive owner borrow through removal or release.
        let entry = unsafe { crate::depot::raw::Proof::new((store, index)) };
        Some(unsafe { Self::new(entry) })
    }
}

impl<E: crate::depot::raw::TagEntry, P: crate::depot::raw::TagFinish<E>> Linked<E, P> {
    pub(in crate::depot) fn into_policy<Q: crate::depot::raw::TagFinish<E>>(self) -> Linked<E, Q> {
        // SAFETY: the consuming transition preserves this same live entry.
        Linked {
            entry: Q::new(self.into_inner()),
            policy: marker::PhantomData,
        }
    }

    pub fn entry(&self) -> &E {
        P::entry(&self.entry)
    }

    pub fn into_inner(self) -> E {
        P::into_inner(self.entry)
    }

    /// # Safety
    /// The caller may change only the payload, preserving the exact entry and its hook.
    pub(in crate::depot) unsafe fn entry_mut(&mut self) -> &mut E {
        P::entry_mut(&mut self.entry)
    }

    pub fn unlink(self) -> tagged::Vacant<E> {
        use tagged::hook::Neighbors;

        Neighbors::new(&self).detach();
        // SAFETY: both adjacent edges were repaired and this exact hook was detached.
        unsafe { tagged::Vacant::new(self.into_inner()) }
    }

    /// # Safety
    /// The entry is the current first node of this endpoint in its retained owner.
    pub(in crate::depot) unsafe fn unlink_front(self, endpoint: NonZero<u32>) -> tagged::Vacant<E> {
        use tagged::hook::Neighbors;

        unsafe { Neighbors::new(&self).detach_front(endpoint) };
        // SAFETY: the exact head was detached and both adjacent edges were repaired.
        unsafe { tagged::Vacant::new(self.into_inner()) }
    }
}

impl<E: crate::depot::raw::TagEntry, P: crate::depot::raw::TagFinish<E>> tagged::Linked<E, P>
where
    E::Store: crate::depot::raw::LaneStore,
{
    pub(in crate::depot) fn get(&self) -> &<E::Store as crate::depot::raw::LaneStore>::Item {
        // SAFETY: the Store contract couples this live membership to its payload.
        unsafe {
            (*<E::Store as crate::depot::raw::LaneStore>::node(self.entry())
                .value
                .get())
            .assume_init_ref()
        }
    }

    pub fn remove(self) -> <E::Store as crate::depot::raw::LaneStore>::Item {
        // SAFETY: unlink retains this exact initialized payload under its exclusive owner.
        unsafe { self.unlink().take() }
    }
}

impl<E: crate::depot::raw::TagEntry> tagged::Linked<E, lanes::Rollback>
where
    E::Store: crate::depot::raw::LaneStore,
{
    pub fn commit(self) {
        self.entry.into_parts();
    }
}
