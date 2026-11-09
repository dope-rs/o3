use std::{cell, mem};

use crate::depot::queue::links;
#[repr(transparent)]
pub struct Free(cell::Cell<Option<links::Index>>);

impl Free {
    pub fn seeded(cap: usize) -> Self {
        Self(cell::Cell::new(
            (cap != 0).then_some(unsafe { links::Index::new_raw(0) }),
        ))
    }

    pub(in crate::depot) fn front(&self) -> Option<links::Index> {
        self.0.get()
    }

    /// # Safety
    /// `index` is this head, preserved by its owner's exclusive reservation.
    pub(in crate::depot) unsafe fn acquire_at<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) {
        let hook = unsafe { hooks.hook_raw(index) };
        let next = hook.next.replace(links::Index::END);
        self.0.set(links::Index::decode(next));
    }

    pub fn recycle(&self, vacant: links::Detached<'_>) {
        let (hook, index) = vacant.into_parts();
        hook.previous.set(index.raw());
        hook.next.set(links::Index::encode(self.0.get()));
        self.0.set(Some(index));
    }
}

const _: () = assert!(mem::size_of::<Free>() == mem::size_of::<u32>());
