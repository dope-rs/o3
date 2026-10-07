use std::num;

use crate::{buffer::resident, mem::credit};

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

    pub fn try_resize(&mut self, capacity: usize) -> Result<(), credit::Error> {
        let core = self.budget.core();
        if let Some(amount) = num::NonZero::new(capacity.saturating_sub(self.capacity)) {
            core.resident.try_acquire(amount, core.capacity)?;
        } else {
            core.resident.refund(self.capacity - capacity);
        }
        self.capacity = capacity;
        Ok(())
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
