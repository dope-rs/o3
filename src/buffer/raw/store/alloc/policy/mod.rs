use std::{convert, num};

pub(in crate::buffer) use debit::{Debit, Token};

mod debit;

pub trait Policy: Sized {
    type Seed: Copy;
    type Failure;
    type Charge: crate::buffer::raw::Token<Self>;

    fn available(seed: Self::Seed) -> usize;

    fn try_acquire(debit: crate::buffer::raw::Debit<Self>) -> Result<Self::Charge, Self::Failure>;

    fn seed(&self) -> Self::Seed;

    fn grow(growth: crate::buffer::raw::Growth<Self>) -> (Self, num::NonZero<u32>);
}

impl Policy for () {
    type Seed = ();
    type Failure = convert::Infallible;
    type Charge = crate::buffer::raw::Debit<Self>;

    fn available((): Self::Seed) -> usize {
        usize::MAX
    }

    fn try_acquire(debit: crate::buffer::raw::Debit<Self>) -> Result<Self::Charge, Self::Failure> {
        Ok(debit)
    }

    fn seed(&self) {
        *self
    }

    fn grow(growth: crate::buffer::raw::Growth<Self>) -> (Self, num::NonZero<u32>) {
        let ((), (), capacity) = growth.into_parts();
        ((), capacity)
    }
}
