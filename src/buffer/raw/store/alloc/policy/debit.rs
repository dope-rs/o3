use std::{mem, num};

use crate::buffer::{resident, write::consume};

pub(in crate::buffer) type Debit<P> = crate::mem::Lease<Refund<P>>;

pub(in crate::buffer) struct Refund<P: crate::buffer::raw::Policy>(P);

impl<P: crate::buffer::raw::Policy> crate::mem::Return for Refund<P> {
    type Item = num::NonZero<u32>;

    fn return_item(self, amount: Self::Item) {
        // SAFETY: only acquired debits construct this private sink, and the
        // sealed lease transfers its unchanged amount exactly once.
        unsafe { self.0.refund(amount) };
    }
}

impl<P: crate::buffer::raw::Policy> crate::mem::Lease<Refund<P>> {
    pub(in crate::buffer) fn try_new(
        policy: P,
        amount: num::NonZero<u32>,
    ) -> Result<Self, P::Failure> {
        policy.try_acquire(amount)?;
        Ok(Self::new(Refund(policy), amount))
    }

    pub(in crate::buffer) fn commit(self) -> P {
        mem::ManuallyDrop::new(self).sink().0
    }
}

impl<'d> crate::mem::Lease<Refund<resident::Budget<'d>>> {
    pub(in crate::buffer) fn try_from_charge(
        charge: resident::Charge<'d>,
    ) -> Result<Option<Self>, consume::Error> {
        consume::Error::check(charge.capacity(), u32::MAX as usize)?;
        let charge = mem::ManuallyDrop::new(charge);
        Ok(num::NonZero::new(charge.capacity() as u32)
            .map(|amount| Self::new(Refund(charge.budget()), amount)))
    }
}

const _: () = assert!(size_of::<Debit<()>>() == size_of::<num::NonZero<u32>>());
const _: () = assert!(size_of::<Debit<resident::Budget<'static>>>() == size_of::<usize>() * 2);
