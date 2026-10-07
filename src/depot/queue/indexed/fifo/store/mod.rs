use std::mem;

pub(in crate::depot::queue::indexed) use front::Front;

use crate::depot::{
    capacity,
    queue::{
        indexed::{fifo, keyed},
        links,
    },
};

mod front;

pub(in crate::depot::queue::indexed) struct Store<T> {
    pub(in crate::depot::queue::indexed) nodes: fifo::Nodes<T, links::Chain<u32>>,
}

impl<T> Store<T> {
    pub(in crate::depot::queue::indexed) fn try_count(
        count: capacity::Count,
    ) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            nodes: fifo::Nodes::try_count(count, links::Chain::new())?,
        })
    }

    pub(in crate::depot::queue::indexed) fn capacity(&self) -> usize {
        self.nodes.count().get()
    }

    pub(in crate::depot::queue::indexed) fn len(&self) -> usize {
        self.nodes.state().len()
    }

    pub(in crate::depot::queue::indexed) fn front_entry(&self) -> Option<Front<'_, T>> {
        if self.nodes.state().is_empty() {
            return None;
        }
        // SAFETY: this store retains the live head in its exact allocation.
        Some(Self::front_from(unsafe {
            crate::depot::raw::Proof::new(self)
        }))
    }

    pub(in crate::depot::queue::indexed) fn front_from(
        source: crate::depot::raw::Proof<&Self, keyed::NonEmpty>,
    ) -> Front<'_, T> {
        let owner = source.into_inner();
        // SAFETY: the retained source proves this exact store's initialized nonempty head.
        let index = unsafe { owner.nodes.state().front_key() };
        unsafe { Front::new(owner.nodes.at(index)) }
    }
}

impl<T> Drop for Store<T> {
    fn drop(&mut self) {
        if !mem::needs_drop::<T>() {
            return;
        }
        let mut next = self.nodes.state().first();
        while let Some(index) = next {
            // SAFETY: the allocation owner retains every initialized node in
            // its unchanged live chain until the backing storage is released.
            let entry = unsafe { self.nodes.at(index) };
            let node = entry.slot();
            next = links::Index::decode(node.hook.next.get());
            unsafe { (*node.value.get()).assume_init_drop() };
        }
    }
}
