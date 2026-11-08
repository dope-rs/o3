use std::{cell, mem, num::NonZero};

use crate::{
    depot,
    depot::queue::{links, tagged},
    depot::raw::TagStore,
};

/// A compact link written only from a node index or a represented endpoint.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(transparent)]
pub(in crate::depot::queue::tagged) struct Link(u32);

impl Link {
    pub(in crate::depot::queue::tagged) const EMPTY: Self = Self(u32::MAX);

    pub(in crate::depot::queue::tagged) const fn node(index: links::Index) -> Self {
        Self(index.raw())
    }

    pub(in crate::depot::queue::tagged) const fn endpoint(endpoint: NonZero<u32>) -> Self {
        Self(endpoint.get())
    }

    fn resolve<S: crate::depot::raw::TagStore>(
        self,
        store: &S,
    ) -> depot::raw::Neighbor<'_, Self, S::Head> {
        if store.is_node(self.0) {
            // SAFETY: a node link is initialized in this immutable domain.
            let index = unsafe { links::Index::new_raw(self.0) };
            depot::raw::Neighbor::node(self, unsafe { store.hook(index) }.parts())
        } else {
            let lane = (u32::MAX - self.0) as usize;
            // SAFETY: linked hooks admit only endpoints minted by their owner's lane.
            depot::raw::Neighbor::head(self, unsafe { store.heads().as_ref().get_unchecked(lane) })
        }
    }
}

impl crate::depot::raw::EndpointWord for Link {
    const EMPTY: Self = Self::EMPTY;

    fn node(index: links::Index) -> Self {
        Self(index.raw())
    }

    fn raw(self) -> u32 {
        self.0
    }
}

/// A node's private compact membership; arbitrary words cannot be written.
#[repr(C)]
pub struct Hook {
    pub(in crate::depot::queue::tagged) previous: cell::Cell<Link>,
    pub(in crate::depot::queue::tagged) next: cell::Cell<Link>,
}

impl Hook {
    pub(in crate::depot::queue::tagged) fn parts(
        &self,
    ) -> crate::depot::raw::EndpointHook<'_, Link> {
        crate::depot::raw::EndpointHook {
            previous: &self.previous,
            next: &self.next,
        }
    }

    pub const fn new(index: links::Index) -> Self {
        Self {
            previous: cell::Cell::new(Link::node(index)),
            next: cell::Cell::new(Link::EMPTY),
        }
    }

    /// # Safety
    /// This hook is detached and belongs to the storage owner's free chain.
    pub(in crate::depot) unsafe fn next(&self) -> u32 {
        self.next.get().0
    }

    pub(in crate::depot) fn is_linked(&self, index: links::Index) -> bool {
        self.previous.get() != Link::node(index)
    }
}

// SAFETY: tagged hooks distinguish detached indices from admitted lane endpoints.
unsafe impl depot::raw::IndexedHook for Hook {
    type Head = links::Chain;
    type Chain = ();
    #[inline]
    fn new(index: links::Index, next: Option<links::Index>) -> Self {
        Self {
            previous: cell::Cell::new(Link::node(index)),
            next: cell::Cell::new(next.map_or(Link::EMPTY, Link::node)),
        }
    }
    fn is_linked(&self, index: links::Index) -> bool {
        self.is_linked(index)
    }
    unsafe fn next(&self) -> u32 {
        unsafe { Self::next(self) }
    }
}

impl<E: crate::depot::raw::TagEntry> tagged::Linked<E> {
    pub fn unlink(self) -> tagged::Vacant<E> {
        let entry = self.entry();
        let store = entry.store();
        let hook = entry.hook();
        let index = entry.index();
        let previous = entry.hook().previous.get();
        let next = entry.hook().next.get();
        let previous = previous.resolve(store);
        let next = next.resolve(store);
        // SAFETY: the retained linked entry owns these exact reciprocal neighbors.
        unsafe { hook.parts().detach(index, previous, next) };
        // SAFETY: both adjacent edges were repaired and this exact hook was detached.
        unsafe { tagged::Vacant::new(self.into_inner()) }
    }

    /// # Safety
    /// The entry is the current first node of this endpoint in its retained owner.
    pub(in crate::depot) unsafe fn unlink_front(self, endpoint: NonZero<u32>) -> tagged::Vacant<E> {
        let entry = self.entry();
        let store = entry.store();
        let hook = entry.hook();
        let index = entry.index();
        let _previous = entry.hook().previous.get();
        let next = entry.hook().next.get();
        // SAFETY: the retained front belongs to this endpoint in its exact owner.
        let lane = unsafe { tagged::Endpoint::new(store, endpoint) };
        let head = lane.head();
        let endpoint = Link::endpoint(lane.endpoint());
        let previous = depot::raw::Neighbor::head(endpoint, head);
        let next = if next == endpoint {
            depot::raw::Neighbor::head(endpoint, head)
        } else {
            // SAFETY: the proven head's non-endpoint successor belongs to its same lane.
            let index = unsafe { links::Index::new_raw(depot::raw::EndpointWord::raw(next)) };
            depot::raw::Neighbor::node(next, unsafe { store.hook(index) }.parts())
        };
        // SAFETY: the retained front fixes its predecessor and both reciprocal neighbors.
        unsafe { hook.parts().detach(index, previous, next) };
        // SAFETY: the exact head was detached and both adjacent edges were repaired.
        unsafe { tagged::Vacant::new(self.into_inner()) }
    }
}

const _: () = assert!(mem::size_of::<Hook>() == 2 * mem::size_of::<u32>());
