use std::ops;

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
    ) -> links::Detached<'a> {
        // SAFETY: the caller retains this exact front, chain, and hook store.
        let change = unsafe { crate::depot::raw::Pop::at(chain, self, index) };
        crate::depot::raw::Removal::commit(change)
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
        let index = free.front()?;
        unsafe { free.acquire_at(self, index) };
        Some(index)
    }
    /// # Safety
    /// The index is detached and absent from every list of this owner.
    /// Implementations return this exact hook to the owner's free list once.
    unsafe fn recycle(&self, free: &links::Free, index: links::Index) {
        // SAFETY: the caller retains this exact hook exclusively outside every list.
        free.recycle(unsafe { links::Detached::new(self.hook_raw(index), index) });
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
        // SAFETY: the original neighbors retain this exact member until this commit.
        unsafe { EndpointHook::from(hook).unlink(index, list, |key| domain.hook_raw(key).into()) };
    }
}

/// One resident value paired with its stable compact hook.
#[derive(Debug)]
#[repr(C)]
pub struct Node<T> {
    value: T,
    hook: links::Hook,
}

impl<T> Node<T> {
    pub fn vacant(value: T, index: u32, next: u32) -> Self {
        Self {
            value,
            hook: links::Hook::vacant(index, next),
        }
    }

    pub const fn hook(&self) -> &links::Hook {
        &self.hook
    }
}

impl<T> ops::Deref for Node<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

// SAFETY: each in-bounds node owns one stable hook for this exact borrowed slice.
unsafe impl<T> Hooks for [Node<T>] {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        unsafe { self.get_unchecked(index.get()) }.hook()
    }
}

mod endpoint;
mod member;
mod mutation;
pub(in crate::depot) use endpoint::{EndpointHook, EndpointWord, Neighbor};
pub(in crate::depot) use member::Removal;
pub(in crate::depot) use mutation::{Detach, Insert, Pop, Splice};

mod lanes;
pub(in crate::depot) use lanes::{LaneInit, LaneStore, LaneUndo};
