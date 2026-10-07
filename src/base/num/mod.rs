use std::{mem, num::NonZero};

use crate::base::marker::ThreadBound;

pub mod bounded;

/// A non-wrapping nonzero identity confined to its constructing thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Epoch(NonZero<u64>, ThreadBound);

impl Epoch {
    pub const INITIAL: Self = Self(NonZero::<u64>::MIN, ThreadBound::NEW);

    pub const fn new(value: NonZero<u64>, thread: ThreadBound) -> Self {
        Self(value, thread)
    }

    pub const fn get(self) -> NonZero<u64> {
        self.0
    }

    pub const fn into_parts(self) -> (NonZero<u64>, ThreadBound) {
        (self.0, self.1)
    }

    #[must_use]
    pub const fn checked_add(self, value: u64) -> Option<Self> {
        match self.0.checked_add(value) {
            Some(next) => Some(Self(next, self.1)),
            None => None,
        }
    }
}

/// Monotonic generation bounded by `MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Generation<const MAX: u32 = { u32::MAX }>(u32);

impl<const MAX: u32> Generation<MAX> {
    pub const INITIAL: Self = Self(0);

    pub const fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn checked_add(self, value: u32) -> Option<Self> {
        let (raw, overflow) = self.0.overflowing_add(value);
        if overflow || raw > MAX {
            None
        } else {
            Some(Self(raw))
        }
    }

    #[must_use]
    pub const fn next(self) -> Option<Self> {
        self.checked_add(1)
    }
}

impl Generation {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

const _: () = {
    assert!(mem::size_of::<Epoch>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Option<Epoch>>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Generation>() == mem::size_of::<u32>());
    assert!(mem::align_of::<Generation>() == mem::align_of::<u32>());
};
