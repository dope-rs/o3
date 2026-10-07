use std::{cell, ptr};

use crate::{
    buffer::{
        resident,
        storage::{
            provided,
            provided::{account, arena},
        },
    },
    mem::rc,
};

/// Only admitted registrations can change this arena's outstanding count.
#[repr(transparent)]
pub(super) struct Counter(cell::Cell<u32>);

impl Counter {
    pub(super) const fn new() -> Self {
        Self(cell::Cell::new(0))
    }
}

/// One arena debit and resident charge retained by its live account edges.
/// Only this owner's Drop returns its debit; other drops preserve that debit
/// through reentrant work between admission and removal.
pub(in crate::buffer) struct Charge<'d> {
    entry: ptr::NonNull<arena::Entry<'d>>,
    pub(super) count: cell::Cell<crate::mem::raw::Live>,
    value: resident::Charge<'d>,
}

impl<'d> Charge<'d> {
    /// Debits once before exposing its sole return authority, with no callbacks.
    /// Admission proves count < limit, so the increment cannot overflow.
    pub(super) fn try_new(
        owner: &provided::Owner<'d>,
        budget: resident::Budget<'d>,
    ) -> Result<Self, account::Cause> {
        let entry = owner.entry;
        // SAFETY: the external owner retains this complete, stable slot.
        let domain = unsafe { entry.as_ref() }.domain;
        let count = domain.accounted.0.get();
        let limit = domain.limit;
        if count >= limit {
            return Err(account::Cause::Exhausted { count, limit });
        }
        // SAFETY: the owner retains the slot's original resident layout.
        let value = budget
            .try_charge(unsafe { arena::Entry::resident(entry) } as usize)
            .map_err(account::Cause::Budget)?;
        domain.accounted.0.set(count + 1);
        Ok(Self {
            entry,
            count: cell::Cell::new(crate::mem::raw::Live::ONE),
            value,
        })
    }
    pub(super) const fn entry(&self) -> ptr::NonNull<arena::Entry<'d>> {
        self.entry
    }
    pub(super) fn budget(&self) -> resident::Budget<'d> {
        self.value.budget()
    }
    pub(in crate::buffer) const fn resident(&self) -> usize {
        self.value.capacity()
    }
    pub(super) fn try_retain(&self) -> Result<(), rc::Error> {
        let next = self.count.get().try_retain()?;
        self.count.set(next);
        Ok(())
    }
}

impl Drop for Charge<'_> {
    fn drop(&mut self) {
        // SAFETY: the registration retains its original slot through removal.
        let domain = unsafe { self.entry.as_ref() }.domain;
        domain.accounted.0.set(domain.accounted.0.get() - 1);
    }
}

const _: () = {
    assert!(size_of::<Charge<'_>>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<Charge<'_>>>() == size_of::<Charge<'_>>());
};
