use crate::depot::queue::links;

/// Stable one-to-one compact-index mapping.
/// # Safety
/// Hooks stay stable/non-aliasing; unchecked nodes are represented.
pub unsafe trait Hooks {
    /// # Safety
    /// The hook is exclusively detached in this exact reciprocal chain and store.
    /// Implementations publish this exact hook once and preserve cardinality.
    unsafe fn chain_push<C: crate::depot::raw::ChainCount>(
        &self,
        chain: &links::Chain<C>,
        index: links::Index,
    ) {
        unsafe { chain.link_back(self, index) };
    }
    /// # Safety
    /// The index is linked in this exact reciprocal chain and store.
    /// Implementations detach this exact hook and preserve cardinality.
    unsafe fn chain_unlink<'a, C: crate::depot::raw::ChainCount>(
        &'a self,
        chain: &'a links::Chain<C>,
        index: links::Index,
    ) -> links::Detached<'a> {
        unsafe { chain.unlink(self, index) }
    }
    /// # Safety
    /// `index` remains the live reciprocal front of this exact chain and hook
    /// store through commit. Implementations detach it and preserve cardinality.
    unsafe fn chain_pop<'a, C: crate::depot::raw::ChainCount>(
        &'a self,
        chain: &'a links::Chain<C>,
        index: links::Index,
    ) -> (links::Edge, links::Detached<'a>) {
        // SAFETY: the caller retains this exact front, chain, and hook store.
        let change = unsafe { crate::depot::raw::Pop::at(chain, self, index) };
        let edge = change.edge();
        (edge, crate::depot::raw::Removal::commit(change))
    }

    /// # Safety
    /// The index is this exact reciprocal cycle's live head.
    /// Implementations preserve this exact cycle and hook store, returning its new live head.
    unsafe fn cycle_step(&self, cycle: &links::Cycle, index: links::Index) -> links::Index {
        unsafe { cycle.next_live(self, index) }
    }

    /// # Safety
    /// The cycle is reciprocal and this exact hook is exclusively detached.
    /// Implementations preserve this exact cycle and hook store.
    unsafe fn cycle_push(&self, cycle: &links::Cycle, index: links::Index) {
        unsafe { cycle.push(self, index) };
    }

    /// # Safety
    /// The index is linked in this exact reciprocal cycle.
    /// Implementations preserve this exact cycle and hook store.
    unsafe fn cycle_unlink(&self, cycle: &links::Cycle, index: links::Index) {
        unsafe { cycle.unlink_key(self, index) };
    }

    /// # Safety
    /// This exclusive free list is acyclic in this exact hook store.
    /// Implementations detach its exact first hook and preserve the remaining list.
    unsafe fn acquire(&self, free: &links::Free) -> Option<links::Index> {
        unsafe { free.acquire_raw(self) }
    }
    /// # Safety
    /// The index is detached and absent from every list of this owner.
    /// Implementations return this exact hook to the owner's free list once.
    unsafe fn recycle(&self, free: &links::Free, index: links::Index) {
        unsafe { free.recycle_now(self, index) }
    }

    /// # Safety
    /// This detached hook and the chain's reciprocal endpoints belong to this exact store.
    unsafe fn push(&self, detached: links::Detached<'_>, chain: &links::Chain) {
        unsafe { detached.push(chain, self) }
    }

    /// # Safety
    /// `index` names a node represented by this view.
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook;
    /// # Safety
    /// `list` and its reciprocal hooks belong to this exact plane; `index` stays exclusively vacant.
    unsafe fn link_raw(&mut self, list: &links::List, index: links::Index) {
        let domain = &*self;
        // SAFETY: the contract retains this vacant hook and its exact reciprocal list.
        let hook = unsafe { domain.hook_raw(index) };
        let adjacent = list.first().map(|next| {
            // SAFETY: this observed head belongs to the same retained hook plane.
            (next, unsafe { domain.hook_raw(next) }.into())
        });
        // SAFETY: all original hooks are resolved before publishing this vacant node.
        unsafe {
            EndpointHook::from(hook).push::<true, _>(list, index, links::Index::END, adjacent)
        };
    }

    /// # Safety
    /// `index` stays exclusively linked in this exact list and reciprocal hook plane.
    unsafe fn unlink_raw(&mut self, list: &links::List, index: links::Index) {
        let domain = &*self;
        // SAFETY: the contract retains this linked hook and its exact reciprocal list.
        let hook = unsafe { domain.hook_raw(index) };
        let prev = hook.previous.get();
        let next = hook.next.get();
        let prior = match links::Index::decode(prev) {
            // SAFETY: this original previous edge is live in the retained plane.
            Some(key) => Neighbor::node(prev, unsafe { domain.hook_raw(key) }.into()),
            None => Neighbor::head(prev, list),
        };
        let after = match links::Index::decode(next) {
            // SAFETY: this original next edge is live in the retained plane.
            Some(key) => Neighbor::node(next, unsafe { domain.hook_raw(key) }.into()),
            None => Neighbor::head(next, list),
        };
        // SAFETY: the original neighbors retain this exact member until this commit.
        unsafe { EndpointHook::from(hook).detach(index, prior, after) };
    }
}

mod endpoint;
mod member;
mod mutation;
pub(in crate::depot) use endpoint::{EndpointHook, EndpointWord, Neighbor};
pub(in crate::depot) use member::{Membership, Removal};
pub(in crate::depot) use mutation::{Detach, Insert, Pop, Splice};

mod lanes;
pub(in crate::depot) use lanes::{LaneInit, LaneStore};
