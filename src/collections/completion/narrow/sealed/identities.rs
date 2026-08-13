use std::{fmt, marker, num, ptr};

use crate::collections::completion::narrow::sealed;

/// A one-word completion key tied to its arena owner.
#[repr(transparent)]
pub struct Key<'owner, T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    pub(super) echo: Echo<T, INDEX_BITS, GENERATION_BITS>,
    pub(super) owner: sealed::Invariant<'owner>,
}

/// An inert one-word identity copied through an external completion source.
#[repr(transparent)]
pub struct Echo<T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    raw: num::NonZeroU64,
    value: marker::PhantomData<fn() -> T>,
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Echo<T, INDEX_BITS, GENERATION_BITS>
{
    pub(super) fn from_entry(entry: ptr::NonNull<sealed::group::Entry<T>>) -> Self {
        // SAFETY: every issued handle refers to a live occupied entry.
        let entry = unsafe { entry.as_ref() };
        let generation = entry.generation.get();
        debug_assert!(generation != 0 && generation <= sealed::generation_max::<GENERATION_BITS>());
        let raw = (u64::from(generation) << INDEX_BITS) | u64::from(entry.index);
        Self {
            // SAFETY: every occupied entry has a nonzero generation in the high bits.
            raw: unsafe { num::NonZeroU64::new_unchecked(raw) },
            value: marker::PhantomData,
        }
    }

    /// Returns inert identity bits suitable for an external echo field.
    pub const fn expose(self) -> u64 {
        self.raw.get()
    }

    /// Reconstructs inert identity bits returned by an external producer.
    pub const fn from_exposed(raw: u64) -> Option<Self> {
        sealed::validate_widths::<INDEX_BITS, GENERATION_BITS>();
        let width = INDEX_BITS + GENERATION_BITS;
        let exceeds_width = match 1u64.checked_shl(width) {
            Some(limit) => raw >= limit,
            None => false,
        };
        if raw == 0 || exceeds_width {
            return None;
        }
        let generation = (raw >> INDEX_BITS) as u32;
        if generation == 0 {
            return None;
        }
        Some(Self {
            // SAFETY: zero was rejected above.
            raw: unsafe { num::NonZeroU64::new_unchecked(raw) },
            value: marker::PhantomData,
        })
    }

    pub(super) const fn index(self) -> u32 {
        let mask = if INDEX_BITS == u32::BITS {
            u32::MAX as u64
        } else {
            (1u64 << INDEX_BITS) - 1
        };
        (self.raw.get() & mask) as u32
    }

    pub(super) const fn generation(self) -> u32 {
        (self.raw.get() >> INDEX_BITS) as u32
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Key<'_, T, INDEX_BITS, GENERATION_BITS>
{
    /// Creates a copyable identity for an external producer.
    pub fn echo(self) -> Echo<T, INDEX_BITS, GENERATION_BITS> {
        self.echo
    }

    /// Exposes the identity in the configured external bit width.
    pub fn expose(self) -> u64 {
        self.echo.expose()
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Clone
    for Echo<T, INDEX_BITS, GENERATION_BITS>
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Copy
    for Echo<T, INDEX_BITS, GENERATION_BITS>
{
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> fmt::Debug
    for Echo<T, INDEX_BITS, GENERATION_BITS>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Echo")
            .field("index", &self.index())
            .field("generation", &self.generation())
            .finish()
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> PartialEq
    for Echo<T, INDEX_BITS, GENERATION_BITS>
{
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Eq
    for Echo<T, INDEX_BITS, GENERATION_BITS>
{
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Clone
    for Key<'_, T, INDEX_BITS, GENERATION_BITS>
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Copy
    for Key<'_, T, INDEX_BITS, GENERATION_BITS>
{
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> fmt::Debug
    for Key<'_, T, INDEX_BITS, GENERATION_BITS>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Key").field(&self.echo).finish()
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> PartialEq
    for Key<'_, T, INDEX_BITS, GENERATION_BITS>
{
    fn eq(&self, other: &Self) -> bool {
        self.echo == other.echo
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Eq
    for Key<'_, T, INDEX_BITS, GENERATION_BITS>
{
}
