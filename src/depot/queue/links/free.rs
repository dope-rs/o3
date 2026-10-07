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
    /// Reachable hooks belong to this store and form an acyclic vacant chain.
    pub(in crate::depot) unsafe fn acquire_raw<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
    ) -> Option<links::Index> {
        let index = self.front()?;
        unsafe { self.acquire_at(hooks, index) };
        Some(index)
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

    /// # Safety
    /// `index` is detached and absent from every live/free chain of this owner.
    pub(in crate::depot) unsafe fn recycle_now<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) {
        // SAFETY: this exact hook is exclusively detached by the caller.
        self.recycle(unsafe { links::Detached::new(hooks.hook_raw(index), index) });
    }
}

const _: () = assert!(mem::size_of::<Free>() == mem::size_of::<u32>());
