use std::{marker, mem};

use crate::{
    alloc::{self, array},
    base::marker::ThreadBound,
    depot::{capacity, queue::links},
};

pub(in crate::depot::queue::round) struct Order<B = ()> {
    storage: array::Block<links::Hook, capacity::Count, links::Cycle>,
    confined: marker::PhantomData<(ThreadBound, B)>,
}

impl<B> Order<B> {
    pub(in crate::depot::queue::round) fn try_new(
        capacity: capacity::Count,
    ) -> Result<Self, alloc::Error> {
        let storage = array::Block::try_new(capacity, links::Cycle::new())?.init(|raw| {
            // SAFETY: Count excludes the sentinel from its slot range.
            let index = unsafe { links::Index::new_usize(raw) };
            links::Hook::new(index)
        });
        Ok(Self {
            storage,
            confined: marker::PhantomData,
        })
    }

    pub(in crate::depot::queue::round) const fn ring(&self) -> &links::Cycle {
        self.storage.state()
    }
}

// SAFETY: the joint owner retains every initialized hook for its full lifetime.
unsafe impl<B> crate::depot::raw::Hooks for Order<B> {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        // SAFETY: the ring/capability retains this exact initialized resident.
        unsafe { self.storage.get_unchecked(index.get()) }
    }
}

const _: () = {
    assert!(mem::size_of::<links::Hook>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Order>() == 2 * mem::size_of::<usize>());
};
