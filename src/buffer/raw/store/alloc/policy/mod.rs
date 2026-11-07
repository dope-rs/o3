use std::{convert, num};

pub(in crate::buffer) use debit::Debit;

mod debit;

pub trait Policy: Copy {
    type Failure;

    fn available(self) -> usize;

    fn try_acquire(self, amount: num::NonZero<u32>) -> Result<(), Self::Failure>;

    /// # Safety
    /// Transfer this policy's exact acquired credit from its owning caller.
    unsafe fn refund(self, amount: num::NonZero<u32>);
}

impl Policy for () {
    type Failure = convert::Infallible;

    fn available(self) -> usize {
        usize::MAX
    }

    fn try_acquire(self, _: num::NonZero<u32>) -> Result<(), Self::Failure> {
        Ok(())
    }

    unsafe fn refund(self, _: num::NonZero<u32>) {}
}
