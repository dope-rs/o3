use std::{cell, mem};

use crate::depot::queue::links;
/// Allocation-free indexed FIFO with an optional cardinality policy.
#[repr(C)]
#[derive(Debug)]
pub struct Chain<C: Copy = ()> {
    head: cell::Cell<u32>,
    tail: cell::Cell<u32>,
    count: cell::Cell<C>,
}

impl<C: crate::depot::raw::ChainCount> Default for Chain<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: crate::depot::raw::ChainCount> Chain<C> {
    pub const fn new() -> Self {
        Self {
            head: cell::Cell::new(links::Index::END),
            tail: cell::Cell::new(links::Index::END),
            count: cell::Cell::new(C::EMPTY),
        }
    }

    pub fn first(&self) -> Option<links::Index> {
        links::Index::decode(self.head.get())
    }

    pub fn front(&self) -> Option<usize> {
        self.first().map(links::Index::get)
    }

    pub fn last(&self) -> Option<links::Index> {
        links::Index::decode(self.tail.get())
    }

    /// # Safety
    /// This exact chain is non-empty for the immediate operation.
    pub(in crate::depot) unsafe fn front_key(&self) -> links::Index {
        unsafe { links::Index::new_raw(self.head.get()) }
    }

    pub fn is_empty(&self) -> bool {
        self.head.get() == links::Index::END
    }

    /// # Safety
    /// `index` is exclusively held outside all chains in this exact store.
    pub(in crate::depot) unsafe fn link_back<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) {
        use crate::depot::raw::Insert;

        // SAFETY: caller supplies this chain's vacant-hook and reciprocal-endpoint proof.
        let change = unsafe { Insert::<C, false>::new(self, hooks, index) };
        Insert::commit(change);
    }

    /// # Safety
    /// `index` is linked in this exact chain and store.
    pub(in crate::depot) unsafe fn unlink<'a, H: crate::depot::raw::Hooks + ?Sized>(
        &'a self,
        hooks: &'a H,
        index: links::Index,
    ) -> links::Detached<'a> {
        use crate::depot::raw::Detach;

        // SAFETY: caller proves index and its reciprocal neighbors belong to this chain/store.
        let change = unsafe { Detach::new(self, hooks, index) };
        crate::depot::raw::Removal::commit(change)
    }
}

impl Chain {
    pub(in crate::depot) fn set_first(&self, index: Option<links::Index>) {
        self.head.set(links::Index::encode(index));
    }

    pub(in crate::depot) fn set_last(&self, index: Option<links::Index>) {
        self.tail.set(links::Index::encode(index));
    }

    /// # Safety
    /// `source` is non-empty, distinct, and both endpoint pairs are reciprocal.
    pub(in crate::depot) unsafe fn append<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        source: &Self,
        hooks: &H,
    ) {
        // SAFETY: caller retains distinct reciprocal endpoint pairs in this exact store.
        let change = unsafe { crate::depot::raw::Splice::new(self, source, hooks) };
        change.commit();
    }

    /// # Safety
    /// `index` is not the target tail and every involved edge is reciprocal.
    pub(in crate::depot) unsafe fn move_back<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        target: &Self,
        hooks: &H,
        index: links::Index,
    ) {
        // SAFETY: caller supplies exact store identity and reciprocal source/target edges.
        let change = unsafe { crate::depot::raw::Relink::new(self, target, hooks, index) };
        change.commit();
    }
}

impl Chain<u32> {
    pub fn len(&self) -> usize {
        self.count.get() as usize
    }
}

const _: () = assert!(mem::size_of::<Chain>() == 2 * mem::size_of::<u32>());

impl<C: Copy> Chain<C> {
    /// # Safety
    /// The operation retains this exact chain; mutations preserve its reciprocal links and cardinality.
    pub(in crate::depot) unsafe fn parts(
        &self,
    ) -> (&cell::Cell<u32>, &cell::Cell<u32>, &cell::Cell<C>) {
        (&self.head, &self.tail, &self.count)
    }
}
