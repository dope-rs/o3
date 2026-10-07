use std::{cell, mem};

use crate::depot::queue::links;

pub(in crate::depot) struct Insert<'a, C: crate::depot::raw::ChainCount, const FRONT: bool> {
    pub(in crate::depot::raw) links: &'a links::Chain<C>,
    edge: links::Edge,
    hook: &'a links::Hook,
    neighbor: &'a cell::Cell<u32>,
}

impl<'a, C: crate::depot::raw::ChainCount, const FRONT: bool> Insert<'a, C, FRONT> {
    /// # Safety
    /// `index` is exclusively held outside all chains; this chain's endpoint is reciprocal in `hooks`.
    pub(in crate::depot) unsafe fn new<H: crate::depot::raw::Hooks + ?Sized>(
        links: &'a links::Chain<C>,
        hooks: &'a H,
        index: links::Index,
    ) -> Self {
        // SAFETY: caller guarantees that index resolves this vacant stable hook.
        let hook = unsafe { hooks.hook_raw(index) };
        // SAFETY: the retained hook is exclusively vacant at this exact index.
        let vacant = unsafe { links::Detached::new(hook, index) };
        // SAFETY: the original contract identifies this exact hook store and chain.
        unsafe { Self::from_vacant(links, hooks, vacant) }
    }

    /// # Safety
    /// `vacant` and this chain's reciprocal endpoints belong to `hooks`.
    pub(in crate::depot) unsafe fn from_vacant<H: crate::depot::raw::Hooks + ?Sized>(
        links: &'a links::Chain<C>,
        hooks: &'a H,
        vacant: links::Detached<'a>,
    ) -> Self {
        let (hook, index) = vacant.into_parts();
        let adjacent = if FRONT {
            (unsafe { links.parts() }).0.get()
        } else {
            (unsafe { links.parts() }).1.get()
        };
        let end = if FRONT {
            (unsafe { links.parts() }).1
        } else {
            (unsafe { links.parts() }).0
        };
        let neighbor = links::Index::decode(adjacent).map_or(end, |index| {
            let hook = unsafe { hooks.hook_raw(index) };
            if FRONT { &hook.previous } else { &hook.next }
        });
        let edge = if FRONT {
            links::Edge::new(index, links::Index::END, adjacent)
        } else {
            links::Edge::new(index, adjacent, links::Index::END)
        };
        Self {
            links,
            edge,
            hook,
            neighbor,
        }
    }

    /// Commits this detached compact position.
    /// Its admission proves this exact chain's count is below `u32::MAX`.
    pub(in crate::depot) fn commit(self) {
        // SAFETY: this admitted insertion retains mutation authority for the same
        // chain; committing its reciprocal links and count preserves that chain.
        let count =
            C::storage(unsafe { self.links.parts() }.2).map(|count| (count, count.get() + 1));
        self.neighbor.set(self.edge.key().raw());
        if FRONT {
            self.hook.previous.set(links::Index::END);
            self.hook.next.set(self.edge.links().1);
            (unsafe { self.links.parts() }).0.set(self.edge.key().raw());
        } else {
            self.hook.previous.set(self.edge.links().0);
            self.hook.next.set(links::Index::END);
            (unsafe { self.links.parts() }).1.set(self.edge.key().raw());
        }
        if let Some((count, next)) = count {
            count.set(next);
        }
    }
}

impl<'a, const FRONT: bool> Insert<'a, (), FRONT> {
    /// # Safety
    /// The hook is exclusively detached and this edge and neighbor match the exact chain and plane until commit.
    pub(in crate::depot) unsafe fn resolved(
        chain: &'a links::Chain,
        edge: links::Edge,
        hook: &'a links::Hook,
        neighbor: Option<&'a links::Hook>,
    ) -> Self {
        let end = if FRONT {
            (unsafe { chain.parts() }).1
        } else {
            (unsafe { chain.parts() }).0
        };
        Self {
            links: chain,
            edge,
            hook,
            neighbor: neighbor.map_or(end, |hook| if FRONT { &hook.previous } else { &hook.next }),
        }
    }
}

const _: () = assert!(mem::size_of::<Insert<'static, (), false>>() == 5 * mem::size_of::<usize>());
