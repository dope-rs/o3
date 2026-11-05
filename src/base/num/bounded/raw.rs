//! Raw constructors for already-proven numeric bounds.

use std::num;

/// Retains a value within its already-proven inclusive bounds.
///
/// # Safety
/// `value` satisfies `MIN..=MAX`.
pub const unsafe fn u32<const MIN: u32, const MAX: u32>(value: u32) -> super::U32<MIN, MAX> {
    super::U32(value)
}

/// Retains a nonzero value within its already-proven inclusive bounds.
///
/// # Safety
/// `MIN..=MAX` is a nonempty nonzero range and `value` satisfies it.
pub const unsafe fn nonzero_u32<const MIN: u32, const MAX: u32>(
    value: num::NonZero<u32>,
) -> super::NonZeroU32<MIN, MAX> {
    let () = super::NonZeroU32::<MIN, MAX>::RANGE;
    super::NonZeroU32(value)
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
