use std::{num, ptr};

use crate::{
    base::cell::region,
    buffer::{resident, resident::Domain},
    mem::{self, credit},
};

/// A one-word authorization naming one exact stable accounting counter.
/// Credits measure retained payload capacity, excluding allocator metadata and padding.
/// Retaining a pool slot can reserve credits without allocating another backing.
#[derive(Clone, Copy, Debug)]
pub struct Budget<'d> {
    pub(super) core: mem::raw::Bound<&'d Domain, region::Proof<region::Identity<'d>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    Unbound,
    Foreign,
}

impl<'d> Budget<'d> {
    pub(in crate::buffer) const fn core(self) -> &'d Domain {
        self.core.source()
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

    pub const fn charge(self) -> resident::Charge<'d> {
        resident::Charge {
            budget: self,
            capacity: 0,
        }
    }

    pub fn try_charge(self, capacity: usize) -> Result<resident::Charge<'d>, credit::Error> {
        if let Some(amount) = num::NonZero::new(capacity) {
            self.core()
                .resident
                .try_acquire(amount, self.core().capacity)?;
        }
        Ok(resident::Charge {
            budget: self,
            capacity,
        })
    }
}

const _: () = assert!(size_of::<Budget<'static>>() == size_of::<usize>());
const _: () = assert!(size_of::<Option<Budget<'static>>>() == size_of::<Budget<'static>>());
