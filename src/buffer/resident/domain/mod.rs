use std::num;

use budget::Budget;
use reuse::Reuse;

pub(in crate::buffer::resident) mod budget;
pub(in crate::buffer::resident) mod errors;
pub(in crate::buffer::resident) mod lease;
pub(in crate::buffer::resident) mod reuse;
pub(in crate::buffer::resident) mod scope;

use crate::{base::cell::region, buffer::resident};

/// One stable resident-accounting counter reused across generative regions.
#[derive(Debug)]
pub struct Domain {
    capacity: usize,
    resident: crate::mem::raw::Account<true>,
}

impl Default for Domain {
    fn default() -> Self {
        Self::new()
    }
}

impl Domain {
    pub const fn new() -> Self {
        Self {
            capacity: 0,
            resident: crate::mem::raw::Account::new(0),
        }
    }

    /// Activates an empty counter for the exact region carried by `proof`.
    pub fn try_budget<'d>(
        &'d mut self,
        capacity: usize,
        proof: region::Proof<region::Identity<'d>>,
    ) -> Result<Budget<'d>, Reuse> {
        if let Some(retained) = num::NonZero::new(self.resident.get()) {
            return Err(Reuse {
                retained,
                previous: self.capacity,
                requested: capacity,
            });
        }
        self.capacity = capacity;
        Ok(Budget {
            core: crate::mem::raw::Bound::new(self, proof),
        })
    }

    pub(in crate::buffer) fn shrink(lease: &mut resident::Lease<'_>, capacity: num::NonZero<u32>) {
        let budget = lease.debit.seed();
        let core = budget.core();
        let amount = lease.debit.amount().get() - capacity.get();
        core.resident.refund(amount as usize);
        lease.debit = crate::buffer::raw::Debit::new(budget, capacity);
    }
}

const _: () = assert!(size_of::<Domain>() == size_of::<usize>() * 2);
