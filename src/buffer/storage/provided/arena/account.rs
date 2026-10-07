use std::ptr;

use storage::{backing, provided};

use crate::{
    buffer,
    buffer::{
        storage,
        storage::{provided::arena, regions},
    },
    mem::rc,
};

/// One edge retaining its concrete initialized registration and scope.
#[derive(Debug)]
pub struct Account<'d> {
    pub(super) charge: ptr::NonNull<arena::Charge<'d>>,
}

impl<'d> Account<'d> {
    pub(super) fn new(charge: &'d arena::Charge<'d>) -> Self {
        // SAFETY: Entry inserted this concrete registration into its own stable
        // slot. Preserve that slot's provenance across every Cell mutation;
        // the typed projection supplies only this registration's address.
        Self {
            charge: charge
                .entry()
                .with_addr(ptr::NonNull::from(charge).addr())
                .cast(),
        }
    }
    pub(in crate::buffer) fn try_clone(&self) -> Result<Self, rc::Error> {
        let charge = self.charge;
        self.charge().try_retain()?;
        Ok(Self { charge })
    }
    pub(in crate::buffer) fn charge(&self) -> &arena::Charge<'d> {
        // SAFETY: the owner borrow retains this exact initialized registration.
        unsafe { self.charge.as_ref() }
    }
}

impl Drop for Account<'_> {
    fn drop(&mut self) {
        let charge = self.charge();
        if let Some(next) = charge.count.get().release() {
            charge.count.set(next);
        } else {
            // SAFETY: the last edge consumes this registration; unregister
            // retires its storage before refunding both original debits.
            let charge = unsafe { self.charge.as_ptr().read() };
            arena::Entry::unregister(charge);
        }
    }
}

impl regions::End for Option<Account<'_>> {
    const EMPTY: Option<Self> = Some(None);
}

impl<'d> backing::Owner<'d> for Option<Account<'d>> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        self.as_ref()
            .map(|owner| backing::Root::Provided(owner.charge()))
    }
}

const _: () = assert!(size_of::<Account<'_>>() == size_of::<usize>());

impl<'d> backing::Owner<'d> for Account<'d> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        Some(backing::Root::Provided(self.charge()))
    }
}

impl buffer::Residency for Option<provided::Account<'_>> {
    fn resident(&self) -> usize {
        self.as_ref().map_or(0, |owner| owner.charge().resident())
    }
}
