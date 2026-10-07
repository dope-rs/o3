use std::{num, pin};

pub use end::End;
pub use loan::Loan;

use crate::{
    base::cell::{brand, region},
    buffer::storage::provided,
};

mod end;
mod loan;

/// Runs with a fixed arena and retains its final reclamation authority.
pub trait Operation<S, T, C> {
    type Output;
    type Terminal;
    type Proof: Release<Self::Terminal, T>;

    fn run<'d>(
        self,
        token: brand::Token<brand::Identity<'d>>,
        regions: (
            region::Token<region::Identity<'d>>,
            region::Token<region::Identity<'d, 1>>,
        ),
        root: provided::Root<'d, T, C, ()>,
        state: pin::Pin<&'d mut S>,
        terminal: Loan<'d, Self::Terminal>,
    ) -> (Self::Output, End<'d, Self::Terminal, Self::Proof>)
    where
        Self: 'd;
}

/// Exact completed operation proof authorizing each remaining item's release.
pub trait Release<T, U> {
    fn release(&self, terminal: &mut T, item: U);
}

/// All slots are occupied or awaiting return to this arena's free list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Full {
    pub capacity: num::NonZero<u32>,
}
