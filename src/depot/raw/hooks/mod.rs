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
        // SAFETY: the contract identifies index as this list's vacant live hook.
        let hook = unsafe { domain.hook_raw(index) };
        let next = links::Index::encode(list.first());
        let after = if next == links::Index::END {
            None
        } else {
            // SAFETY: the list head is a live hook in the same domain by the
            // caller's reciprocal-topology invariant.
            Some(unsafe { domain.hook_raw(links::Index::new_raw(next)) })
        };
        hook.previous.set(links::Index::END);
        hook.next.set(next);
        list.set(Some(index));
        if let Some(after) = after {
            after.previous.set(index.raw());
        }
    }

    /// # Safety
    /// `index` stays exclusively linked in this exact list and reciprocal hook plane.
    unsafe fn unlink_raw(&mut self, list: &links::List, index: links::Index) {
        let domain = &*self;
        // SAFETY: the contract identifies index as this list's linked live hook.
        let hook = unsafe { domain.hook_raw(index) };
        let prev = hook.previous.get();
        let next = hook.next.get();
        let prior = if prev == links::Index::END {
            None
        } else {
            // SAFETY: a non-END previous edge is live and reciprocal by contract.
            Some(unsafe { domain.hook_raw(links::Index::new_raw(prev)) })
        };
        let after = if next == links::Index::END {
            None
        } else {
            // SAFETY: a non-END next edge is live and reciprocal by contract.
            Some(unsafe { domain.hook_raw(links::Index::new_raw(next)) })
        };
        hook.previous.set(index.raw());
        hook.next.set(links::Index::END);
        if let Some(prior) = prior {
            prior.next.set(next);
        } else {
            list.set(links::Index::decode(next));
        }
        if let Some(after) = after {
            after.previous.set(prev);
        }
    }
}

mod endpoint;
mod member;
mod mutation;
pub(in crate::depot) use endpoint::{EndpointHook, EndpointWord, Neighbor};
pub(in crate::depot) use member::{Membership, Removal};
pub(in crate::depot) use mutation::{Detach, Insert, Pop, Relink, Splice};

mod lanes;
pub(in crate::depot) use lanes::{LaneInit, LaneStore};
