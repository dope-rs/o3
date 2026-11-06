use std::{cell, mem};

use crate::depot::queue::links;

pub(in crate::depot) struct Insert<'a, C: crate::depot::raw::ChainCount> {
    pub(in crate::depot::raw) links: &'a links::Chain<C>,
    index: links::Index,
    previous: u32,
    hook: &'a links::Hook,
    neighbor: &'a cell::Cell<u32>,
}

impl<'a, C: crate::depot::raw::ChainCount> Insert<'a, C> {
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
        let previous = (unsafe { links.parts() }).1.get();
        let neighbor = links::Index::decode(previous)
            .map_or((unsafe { links.parts() }).0, |index| {
                &unsafe { hooks.hook_raw(index) }.next
            });
        Self {
            links,
            index,
            previous,
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
        self.neighbor.set(self.index.raw());
        self.hook.previous.set(self.previous);
        self.hook.next.set(links::Index::END);
        (unsafe { self.links.parts() }).1.set(self.index.raw());
        if let Some((count, next)) = count {
            count.set(next);
        }
    }
}

const _: () = assert!(mem::size_of::<Insert<'static, ()>>() == 4 * mem::size_of::<usize>());
