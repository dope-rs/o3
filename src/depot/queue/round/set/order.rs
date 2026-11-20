use std::{marker, mem};

use crate::{
    alloc::{self, array},
    base::marker::ThreadBound,
    depot::{arena::slab::key, capacity, queue::links},
};

pub(in crate::depot::queue::round) struct Order<'h, Tag> {
    storage: array::Block<links::Hook, capacity::Count, links::Cycle>,
    confined: marker::PhantomData<(ThreadBound, capacity::Key<'h, key::Handle<Tag>>)>,
}

impl<'h, Tag> Order<'h, Tag> {
    pub(super) fn try_in(domain: capacity::Domain<'h>) -> Result<Self, alloc::Error> {
        let storage = array::Block::try_new(domain.count(), links::Cycle::new())?.init(|raw| {
            // SAFETY: Count excludes the sentinel from its slot range.
            let index = unsafe { links::Index::new_usize(raw) };
            links::Hook::new(index)
        });
        Ok(Self {
            storage,
            confined: marker::PhantomData,
        })
    }

    pub(super) const fn ring(&self) -> &links::Cycle {
        self.storage.state()
    }
}

// SAFETY: the joint owner retains every initialized hook for its full lifetime.
unsafe impl<Tag> crate::depot::raw::Hooks for Order<'_, Tag> {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        // SAFETY: the ring/capability retains this exact initialized resident.
        unsafe { self.storage.get_unchecked(index.get()) }
    }
}

const _: () = {
    assert!(mem::size_of::<links::Hook>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Order<'static, ()>>() == 2 * mem::size_of::<usize>());
};
