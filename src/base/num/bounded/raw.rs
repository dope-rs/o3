//! Raw range witnesses for the bounded numeric constructors.

use std::num;

/// One value assumed to lie inside the inclusive `MIN..=MAX` bounds.
#[derive(Clone, Copy, Debug)]
pub struct Proof<const MIN: u32, const MAX: u32>(u32);

impl<const MIN: u32, const MAX: u32> Proof<MIN, MAX> {
    /// Assumes `value` lies inside these exact bounds.
    ///
    /// # Safety
    /// `value` satisfies `MIN..=MAX`.
    pub const unsafe fn assume(value: u32) -> Self {
        Self(value)
    }

    /// Retains the assumed value as its bounded integer.
    pub const fn retain(self) -> super::U32<MIN, MAX> {
        super::U32(self.0)
    }
}

/// One nonzero value assumed to lie inside the inclusive `MIN..=MAX` bounds.
#[derive(Clone, Copy, Debug)]
pub struct NonZeroProof<const MIN: u32, const MAX: u32>(num::NonZero<u32>);

impl<const MIN: u32, const MAX: u32> NonZeroProof<MIN, MAX> {
    /// Assumes `value` lies inside these exact bounds.
    ///
    /// # Safety
    /// `MIN..=MAX` is a nonempty nonzero range and `value` satisfies it.
    pub const unsafe fn assume(value: num::NonZero<u32>) -> Self {
        let () = super::NonZeroU32::<MIN, MAX>::RANGE;
        Self(value)
    }

    /// Retains the assumed value as its bounded nonzero integer.
    pub const fn retain(self) -> super::NonZeroU32<MIN, MAX> {
        super::NonZeroU32(self.0)
    }
}

/// Retains an already-proven nonzero count within its exact upper bound.
///
/// # Safety
/// `value` satisfies `1..=MAX`.
pub(crate) const unsafe fn nonzero_usize<const MAX: usize>(
    value: usize,
) -> super::NonZeroUsize<1, MAX> {
    let () = super::NonZeroUsize::<1, MAX>::RANGE;
    // SAFETY: the caller proves value is nonzero and no greater than MAX.
    super::NonZeroUsize(unsafe { num::NonZero::new_unchecked(value) })
}
