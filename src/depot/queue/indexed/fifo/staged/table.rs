use std::mem;

use crate::{
    alloc::{self, array},
    depot::{
        capacity,
        queue::{indexed::fifo::staged, links},
    },
};

/// Indexed FIFO retaining each payload's phase in its owning slot.
#[repr(C)]
pub struct Table<K, Q, P, I: crate::depot::raw::Compact = u32> {
    pub(in crate::depot) nodes: array::Block<
        crate::depot::raw::StagedNode<K, Q, P>,
        capacity::Count,
        crate::depot::raw::StagedState<I>,
    >,
}

impl<K, Q, P, I: crate::depot::raw::Compact> Table<K, Q, P, I> {
    pub const SLOT_BYTES: usize = mem::size_of::<crate::depot::raw::StagedNode<K, Q, P>>();

    pub fn try_new(capacity: usize) -> Result<Self, alloc::Error> {
        Self::try_count(super::super::Counted::try_new(capacity)?)
    }

    pub fn try_count(capacity: impl crate::depot::raw::Count<I>) -> Result<Self, alloc::Error> {
        use alloc::array::Block;

        use links::Index;

        let nodes = Block::try_new(capacity.count(), crate::depot::raw::StagedState::new())?.init(
            |index| {
                let index = unsafe { Index::new_usize(index) };
                crate::depot::raw::StagedNode::new(index)
            },
        );
        Ok(Self { nodes })
    }

    pub fn get(&self, index: I) -> Option<staged::State<'_, K>> {
        Some(self.nodes.get(index.into_raw() as usize)?.state())
    }

    pub fn parked(&self, index: I) -> Option<(&K, &P)> {
        self.nodes.get(index.into_raw() as usize)?.parked()
    }

    pub fn entry(&mut self, index: I) -> Option<staged::Entry<'_, K, Q, P, I>> {
        let member = staged::Member::get(self, index)?;
        // SAFETY: the original mutable table loan remains exclusive for the entry.
        Some(staged::Entry::from_source(unsafe {
            crate::depot::raw::Proof::new(member)
        }))
    }

    /// Borrows a vacancy only when the represented slot is uninitialized.
    pub fn vacant(&mut self, index: I) -> Option<staged::Slot<'_, K, Q, P, (), I>> {
        match self.entry(index)? {
            staged::Entry::Vacant(entry) => Some(entry),
            staged::Entry::Queued(_) | staged::Entry::Parked(_) => None,
        }
    }

    pub fn front(&mut self) -> Option<staged::Slot<'_, K, Q, P, staged::Queued, I>> {
        staged::Slot::first(unsafe { crate::depot::raw::Proof::new(&*self) })
    }

    pub fn clear(&mut self) {
        while let Some(entry) = self.front() {
            entry.clear();
        }
        while let Some(slot) = staged::Slot::<_, _, _, staged::Parked, _>::first(unsafe {
            crate::depot::raw::Proof::new(&*self)
        }) {
            slot.clear();
        }
    }

    pub fn capacity(&self) -> capacity::Count {
        *self.nodes.extent()
    }

    pub fn len(&self) -> usize {
        self.nodes.state().occupied.get() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K, Q, P, I: crate::depot::raw::Compact> Drop for Table<K, Q, P, I> {
    fn drop(&mut self) {
        if mem::needs_drop::<K>() || mem::needs_drop::<Q>() {
            let mut next = self.nodes.state().head.get();
            while let Some(index) = next {
                let node = unsafe { crate::depot::raw::StagedNode::at(self, index) };
                next = node.next.get();
                unsafe { (&mut *node.value.get()).assume_init_mut().clear_queued() };
            }
        }
        if mem::needs_drop::<K>() || mem::needs_drop::<P>() {
            let mut next = self.nodes.state().parked.get();
            while let Some(index) = next {
                let node = unsafe { crate::depot::raw::StagedNode::at(self, index) };
                next = node.next.get();
                unsafe { (&mut *node.value.get()).assume_init_mut().clear_parked() };
            }
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Table<u64, u64, [u64; 5]>>() == 4 * mem::size_of::<usize>());
    assert!(Table::<u64, [u64; 2], [u64; 5]>::SLOT_BYTES == 8 * mem::size_of::<u64>());
};
