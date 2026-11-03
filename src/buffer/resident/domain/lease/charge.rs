use std::{mem, num};

use crate::{
    buffer::{resident, write::consume},
    mem::credit,
};

/// One exact resident debit. The lifetime cannot be detached by safe code.
pub struct Charge<'d> {
    pub(in crate::buffer::resident) budget: resident::Budget<'d>,
    pub(in crate::buffer::resident) capacity: usize,
}

impl<'d> Charge<'d> {
    pub(in crate::buffer) const fn budget(&self) -> resident::Budget<'d> {
        self.budget
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Transfers part of this debit without changing the domain's resident total.
    /// An oversized request leaves this charge unchanged.
    pub fn try_split(&mut self, capacity: usize) -> Result<Self, credit::Error> {
        let Some(remaining) = self.capacity.checked_sub(capacity) else {
            return Err(credit::Error::new(
                num::NonZero::<usize>::MIN.saturating_add(capacity - 1),
                self.capacity,
            ));
        };
        self.capacity = remaining;
        Ok(Self {
            budget: self.budget,
            capacity,
        })
    }

    pub(in crate::buffer) fn try_lease(
        self,
    ) -> Result<Option<resident::Lease<'d>>, consume::Error> {
        consume::Error::check(self.capacity, u32::MAX as usize)?;
        // The lease takes this exact debit; a zero charge has no refund obligation.
        let charge = mem::ManuallyDrop::new(self);
        Ok(
            num::NonZero::new(charge.capacity as u32).map(|capacity| resident::Lease {
                debit: crate::buffer::raw::Debit::new(charge.budget, capacity),
            }),
        )
    }

    pub fn try_resize(&mut self, capacity: usize) -> Result<(), credit::Error> {
        let core = self.budget.core();
        if let Some(amount) = num::NonZero::new(capacity.saturating_sub(self.capacity)) {
            core.resident.try_acquire(amount, core.capacity)?;
            self.capacity = capacity;
        } else {
            self.truncate(capacity);
        }
        Ok(())
    }

    pub(in crate::buffer) fn truncate(&mut self, capacity: usize) {
        let capacity = capacity.min(self.capacity);
        self.budget.core().resident.refund(self.capacity - capacity);
        self.capacity = capacity;
    }
}

impl Drop for Charge<'_> {
    fn drop(&mut self) {
        let core = self.budget.core();
        core.resident.refund(self.capacity);
    }
}

const _: () = assert!(size_of::<Charge<'static>>() == size_of::<usize>() * 2);
const _: () = assert!(size_of::<Option<Charge<'static>>>() == size_of::<Charge<'static>>());
