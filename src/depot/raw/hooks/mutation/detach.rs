use std::{cell, mem};

use crate::depot::queue::links;

pub(in crate::depot) struct Detach<'a, C: crate::depot::raw::ChainCount> {
    pub(in crate::depot::raw) links: &'a links::Chain<C>,
    index: links::Index,
    previous_index: u32,
    next_index: u32,
    hook: &'a links::Hook,
    previous: &'a cell::Cell<u32>,
    next: &'a cell::Cell<u32>,
}

impl<'a, C: crate::depot::raw::ChainCount> Detach<'a, C> {
    /// # Safety
    /// `index` is linked with reciprocal neighbors in this exact chain and store.
    pub(in crate::depot) unsafe fn new<H: crate::depot::raw::Hooks + ?Sized>(
        links: &'a links::Chain<C>,
        hooks: &'a H,
        index: links::Index,
    ) -> Self {
        // SAFETY: caller identifies index as a live linked hook in this store.
        let hook = unsafe { hooks.hook_raw(index) };
        let previous = hook.previous.get();
        let next = hook.next.get();
        // SAFETY: both retained neighbors belong to this exact chain and store.
        let (prior, after) = unsafe { Self::neighbors(links, hooks, previous, next) };
        Self {
            links,
            index,
            previous_index: previous,
            next_index: next,
            hook,
            previous: prior,
            next: after,
        }
    }

    /// # Safety
    /// Both indices identify this chain's retained neighbors or their endpoints.
    unsafe fn neighbors<H: crate::depot::raw::Hooks + ?Sized>(
        links: &'a links::Chain<C>,
        hooks: &'a H,
        previous: u32,
        next: u32,
    ) -> (&'a cell::Cell<u32>, &'a cell::Cell<u32>) {
        let prior = links::Index::decode(previous).map_or((unsafe { links.parts() }).0, |index| {
            &unsafe { hooks.hook_raw(index) }.next
        });
        let after = links::Index::decode(next).map_or((unsafe { links.parts() }).1, |index| {
            &unsafe { hooks.hook_raw(index) }.previous
        });
        (prior, after)
    }
}

impl<'a> Detach<'a, ()> {
    /// # Safety
    /// The original hook is occupied again under the same exclusive owner;
    /// its unchanged neighbors still bound the gap left by its removal.
    pub(in crate::depot) unsafe fn restore<H: crate::depot::raw::Hooks + ?Sized>(
        chain: &'a links::Chain,
        hooks: &'a H,
        index: links::Index,
    ) {
        // SAFETY: the caller restored this exact hook and retained both neighbors.
        let hook = unsafe { hooks.hook_raw(index) };
        let (previous, next) =
            unsafe { Self::neighbors(chain, hooks, hook.previous.get(), hook.next.get()) };
        previous.set(index.raw());
        next.set(index.raw());
    }

    /// # Safety
    /// The exact chain owns this live hook and reciprocal neighbors exclusively until commit.
    pub(in crate::depot) unsafe fn resolved(
        chain: &'a links::Chain,
        edge: links::Edge,
        hook: &'a links::Hook,
        previous: Option<&'a links::Hook>,
        next: Option<&'a links::Hook>,
    ) -> Self {
        Self {
            links: chain,
            index: edge.key(),
            previous_index: edge.links().0,
            next_index: edge.links().1,
            hook,
            previous: previous.map_or((unsafe { chain.parts() }).0, |hook| &hook.next),
            next: next.map_or((unsafe { chain.parts() }).1, |hook| &hook.previous),
        }
    }
}

const _: () = assert!(mem::size_of::<Detach<'static, ()>>() == 6 * mem::size_of::<usize>());

// SAFETY: construction retains this exact chain's live hook until consumption.
unsafe impl<'a, C: crate::depot::raw::ChainCount> crate::depot::raw::Removal<'a, C>
    for Detach<'a, C>
{
    fn links(&self) -> &'a links::Chain<C> {
        self.links
    }

    unsafe fn apply(self) -> links::Detached<'a> {
        self.previous.set(self.next_index);
        self.next.set(self.previous_index);
        // SAFETY: committing these reciprocal links detached this exact retained hook.
        unsafe { links::Detached::new(self.hook, self.index) }
    }
}
