//! Bounded count proofs for storage conformance tests.
use std::num::NonZero;

use o3::{depot, depot::capacity};

use crate::common::depot::index::Zero;

#[derive(Clone, Copy, Debug)]
pub struct Single;

// SAFETY: the sole represented position in this one-slot extent is zero.
unsafe impl depot::raw::Count<Zero> for Single {
    const MAX: u32 = 1;

    fn count(self) -> capacity::Count {
        capacity::Count::new(1)
    }
}

/// A fixture count retaining its exact value and static upper bound, including zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Count<const N: u32>(capacity::Count);

impl<const N: u32> Count<N> {
    pub const FULL: Self = Self(capacity::Count::new(N));

    pub const fn try_new(count: capacity::Count) -> Result<Self, capacity::Shortage> {
        if count.raw() > N {
            // SAFETY: a count exceeding a nonnegative bound is nonzero.
            let needed = unsafe { NonZero::new_unchecked(count.get()) };
            return Err(capacity::Shortage {
                requested: needed,
                available: N as usize,
            });
        }
        Ok(Self(count))
    }
}

// SAFETY: construction checks the bound and every copy retains the same exact count.
unsafe impl<const N: u32> depot::raw::Count for Count<N> {
    const MAX: u32 = N;

    fn count(self) -> capacity::Count {
        self.0
    }
}

const _: () = assert!(size_of::<Count<3>>() == size_of::<u32>());
