use std::num;

use crate::buffer::{
    pool,
    resident::staging::{Error, Plan},
};

/// Staging payload-capacity ceiling and fixed pool geometry for one connection set.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Layout {
    plan: Plan,
    warm_slots: num::NonZero<u32>,
    accounts: u32,
    resident: num::NonZero<usize>,
}

impl Layout {
    pub fn new(buffers: usize, plan: Plan, shared: usize, reserve: usize) -> Result<Self, Error> {
        let Some(buffers) = num::NonZero::new(buffers) else {
            return Err(Error::ZeroBuffers);
        };
        let slots = buffers
            .checked_add(shared)
            .ok_or(Error::SlotsOverflow { buffers, shared })?;
        let slots = num::NonZero::<u32>::try_from(slots).map_err(|cause| Error::SlotsTooLarge {
            buffers,
            shared,
            cause,
        })?;
        let accounts = shared as u32;
        let pool = pool::Layout::try_capacity(slots.get(), plan.warm()).map_err(Error::Pool)?;
        let warm = pool.bytes();
        let limit = num::NonZero::<usize>::MIN.saturating_add(plan.ceiling().get() as usize - 1);
        let resident = buffers
            .checked_mul(limit)
            .and_then(|resident| resident.checked_add(warm))
            .and_then(|resident| resident.checked_add(reserve))
            .ok_or(Error::ResidentOverflow {
                buffers,
                plan,
                shared,
                reserve,
            })?;
        Ok(Self {
            plan,
            warm_slots: slots,
            accounts,
            resident,
        })
    }

    pub const fn plan(self) -> Plan {
        self.plan
    }

    /// Required payload credits; fixed pool metadata and allocator padding are separate.
    pub const fn resident(self) -> num::NonZero<usize> {
        self.resident
    }

    pub(in crate::buffer) const fn warm_slots(self) -> num::NonZero<u32> {
        self.warm_slots
    }

    pub(in crate::buffer) const fn accounts(self) -> u32 {
        self.accounts
    }
}

const _: () = assert!(size_of::<Layout>() == size_of::<usize>() * 3);
