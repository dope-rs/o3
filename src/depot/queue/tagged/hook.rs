use std::{cell, mem, num::NonZero};

use crate::{
    depot,
    depot::queue::{links, tagged},
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
        if (self.0 as usize) < store.capacity() {
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

    pub(in crate::depot) fn free(index: links::Index, next: Option<links::Index>) -> Self {
        Self {
            previous: cell::Cell::new(Link::node(index)),
            next: cell::Cell::new(next.map_or(Link::EMPTY, Link::node)),
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

pub(in crate::depot::queue::tagged) struct Neighbors<'a, S: crate::depot::raw::TagStore> {
    store: &'a S,
    hook: &'a Hook,
    index: links::Index,
    previous: Link,
    next: Link,
}

impl<'a, S: crate::depot::raw::TagStore> Neighbors<'a, S> {
    pub(in crate::depot::queue::tagged) fn new<
        E: crate::depot::raw::TagEntry<Store = S>,
        P: crate::depot::raw::TagFinish<E>,
    >(
        entry: &'a tagged::Linked<E, P>,
    ) -> Self {
        let entry = entry.entry();
        Self {
            store: entry.store(),
            hook: entry.hook(),
            index: entry.index(),
            previous: entry.hook().previous.get(),
            next: entry.hook().next.get(),
        }
    }

    pub(in crate::depot::queue::tagged) fn detach(self) {
        let previous = self.previous.resolve(self.store);
        let next = self.next.resolve(self.store);
        // SAFETY: the retained linked entry owns these exact reciprocal neighbors.
        unsafe { self.hook.parts().detach(self.index, previous, next) };
    }

    /// # Safety
    /// The retained linked entry is the current first node of this exact endpoint.
    pub(in crate::depot::queue::tagged) unsafe fn detach_front(self, endpoint: NonZero<u32>) {
        // SAFETY: the retained front belongs to this endpoint in its exact owner.
        let lane = unsafe { tagged::Endpoint::new(self.store, endpoint) };
        let head = lane.head();
        let endpoint = Link::endpoint(lane.endpoint());
        let previous = depot::raw::Neighbor::head(endpoint, head);
        let next = if self.next == endpoint {
            depot::raw::Neighbor::head(endpoint, head)
        } else {
            // SAFETY: the proven head's non-endpoint successor belongs to its same lane.
            let index = unsafe { links::Index::new_raw(depot::raw::EndpointWord::raw(self.next)) };
            depot::raw::Neighbor::node(self.next, unsafe { self.store.hook(index) }.parts())
        };
        // SAFETY: the retained front fixes its predecessor and both reciprocal neighbors.
        unsafe { self.hook.parts().detach(self.index, previous, next) };
    }
}

const _: () = assert!(mem::size_of::<Hook>() == 2 * mem::size_of::<u32>());
