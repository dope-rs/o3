use std::{mem, num};

pub(in crate::buffer::resident) mod charge;

use crate::{buffer::resident, mem::credit};

impl<'d> crate::buffer::raw::Token<Lease<'d>> for Lease<'d> {
    fn debit(&self) -> &crate::buffer::raw::Debit<Lease<'d>> {
        &self.debit
    }

    fn commit(self) -> (Lease<'d>, num::NonZero<u32>) {
        let amount = self.debit.amount();
        (self, amount)
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let core = self.debit.seed().core();
        core.resident.refund(self.debit.amount().get() as usize);
    }
}

#[derive(Debug)]
pub struct Lease<'d> {
    pub(in crate::buffer::resident) debit: crate::buffer::raw::Debit<Self>,
}

impl<'d> crate::buffer::raw::Policy for Lease<'d> {
    type Seed = resident::Budget<'d>;
    type Failure = credit::Error;
    type Charge = Self;

    fn available(seed: Self::Seed) -> usize {
        seed.available()
    }

    fn try_acquire(debit: crate::buffer::raw::Debit<Self>) -> Result<Self::Charge, Self::Failure> {
        let core = debit.seed().core();
        let amount = num::NonZero::<usize>::MIN.saturating_add(debit.amount().get() as usize - 1);
        core.resident.try_acquire(amount, core.capacity)?;
        Ok(Self { debit })
    }

    fn seed(&self) -> Self::Seed {
        self.debit.seed()
    }

    fn grow(growth: crate::buffer::raw::Growth<Self>) -> (Self, num::NonZero<u32>) {
        let (source, added, capacity) = growth.into_parts();
        let owners = mem::ManuallyDrop::new((source, added));
        let policy = Self {
            debit: crate::buffer::raw::Debit::new(owners.0.debit.seed(), capacity),
        };
        (policy, capacity)
    }
}

const _: () =
    assert!(size_of::<crate::buffer::raw::Debit<Lease<'static>>>() == size_of::<usize>() * 2);
const _: () = assert!(size_of::<Lease<'static>>() == size_of::<usize>() * 2);
const _: () = assert!(size_of::<Option<Lease<'static>>>() == size_of::<Lease<'static>>());
const _: () =
    assert!(size_of::<crate::buffer::raw::Header<Lease<'static>>>() == size_of::<usize>() * 3);
