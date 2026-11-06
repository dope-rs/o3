use std::{num, ptr};

use crate::{
    base::cell::region,
    buffer::resident::{Domain, Reuse},
    mem::credit,
};

/// A one-word authorization naming one exact stable accounting counter.
/// Credits measure retained payload capacity, excluding allocator metadata and padding.
/// Retaining a pool slot can reserve credits without allocating another backing.
#[derive(Clone, Copy, Debug)]
pub struct Budget<'d> {
    core: (&'d Domain, region::Proof<region::Identity<'d>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    Unbound,
    Foreign,
}

impl Domain {
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
            core: (self, proof),
        })
    }
}

impl<'d> Budget<'d> {
    pub(in crate::buffer) const fn core(self) -> &'d Domain {
        self.core.0
    }

    pub(in crate::buffer) fn try_bind(self, domain: &mut Option<Self>) -> Result<(), BindError> {
        match domain {
            Some(bound) if !ptr::eq(bound.core(), self.core()) => Err(BindError::Foreign),
            Some(_) => Ok(()),
            None => {
                *domain = Some(self);
                Ok(())
            }
        }
    }

    pub fn capacity(self) -> usize {
        self.core().capacity
    }

    pub fn resident(self) -> usize {
        self.core().resident.get()
    }

    pub fn available(self) -> usize {
        self.capacity() - self.resident()
    }

    pub const fn charge(self) -> Charge<'d> {
        Charge {
            budget: self,
            capacity: 0,
        }
    }

    pub fn try_charge(self, capacity: usize) -> Result<Charge<'d>, credit::Error> {
        if let Some(amount) = num::NonZero::new(capacity) {
            self.core()
                .resident
                .try_acquire(amount, self.core().capacity)?;
        }
        Ok(Charge {
            budget: self,
            capacity,
        })
    }
}

const _: () = assert!(size_of::<Budget<'static>>() == size_of::<usize>());
const _: () = assert!(size_of::<Option<Budget<'static>>>() == size_of::<Budget<'static>>());

/// One exact resident debit. The lifetime cannot be detached by safe code.
pub struct Charge<'d> {
    budget: Budget<'d>,
    capacity: usize,
}

impl<'d> Charge<'d> {
    pub(in crate::buffer) const fn budget(&self) -> Budget<'d> {
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

mod raw {
    use super::{Budget, credit, num};

    impl crate::buffer::raw::Policy for Budget<'_> {
        type Failure = credit::Error;

        fn available(self) -> usize {
            self.available()
        }

        fn try_acquire(self, amount: num::NonZero<u32>) -> Result<(), Self::Failure> {
            let core = self.core();
            let amount = num::NonZero::<usize>::MIN.saturating_add(amount.get() as usize - 1);
            core.resident.try_acquire(amount, core.capacity)
        }

        unsafe fn refund(self, amount: num::NonZero<u32>) {
            self.core().resident.refund(amount.get() as usize);
        }
    }
}
