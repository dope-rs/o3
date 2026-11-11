use std::{mem, num::NonZero};

pub use error::Error;
pub use stale::Stale;

use crate::depot::{arena::slab::external, capacity};

pub mod cycle;
mod error;
pub(in crate::depot) mod handle;
mod stale;

/// The physical observation paired with the original branded key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation<const MAX: u32 = { u32::MAX }, B = (), R = ()> {
    Index { capacity: capacity::Count },
    Free { actual: Generation<MAX> },
    Reserved { actual: Generation<MAX> },
    Retired { actual: Generation<MAX>, retired: R },
    Generation { actual: Generation<MAX> },
    Busy { actual: Generation<MAX>, borrow: B },
}

const _: () = assert!(mem::size_of::<Observation>() == 2 * mem::size_of::<u32>());

pub type Handle<Tag = (), const MAX: u32 = { u32::MAX }> = handle::Handle<Tag, MAX, false>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Parts<const MAX: u32 = { u32::MAX }> {
    raw: NonZero<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Generation<const MAX: u32 = { u32::MAX }>(NonZero<u32>);

impl<const MAX: u32> Generation<MAX> {
    pub const MIN: Self = {
        assert!(MAX != 0, "generation limit must be nonzero");
        Self(NonZero::<u32>::MIN)
    };

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// The retained generation as its nonzero wire value, without a second check.
    pub const fn nonzero(self) -> NonZero<u32> {
        self.0
    }

    #[must_use]
    pub const fn checked_add(self, value: u32) -> Option<Self> {
        match self.0.checked_add(value) {
            Some(next) if next.get() <= MAX => Some(Self(next)),
            _ => None,
        }
    }
}

impl<const MAX: u32> external::Generation for Generation<MAX> {
    const INITIAL: Self = Self::MIN;

    fn next(self) -> Option<Self> {
        self.checked_add(1)
    }
}

impl<const MAX: u32> Parts<MAX> {
    pub const fn new(index: u32, epoch: Generation<MAX>) -> Self {
        let raw = ((epoch.get() as u64) << 32) | index as u64;
        // SAFETY: the retained generation is nonzero, so the shifted epoch keeps the word nonzero.
        Self {
            raw: unsafe { NonZero::<u64>::new_unchecked(raw) },
        }
    }

    #[must_use]
    pub const fn from_raw(raw: NonZero<u64>) -> Option<Self> {
        let epoch = (raw.get() >> 32) as u32;
        if epoch == 0 || epoch > MAX {
            return None;
        }
        let _ = Generation::<MAX>::MIN;
        Some(Self { raw })
    }

    pub const fn into_raw(self) -> NonZero<u64> {
        self.raw
    }

    /// Widens the generation domain while retaining the packed identity.
    pub const fn widen<const TO: u32>(self) -> Parts<TO> {
        const { assert!(MAX <= TO, "bounds") };
        Parts { raw: self.raw }
    }

    pub const fn index(self) -> u32 {
        self.raw.get() as u32
    }

    pub const fn generation(self) -> Generation<MAX> {
        let _ = Generation::<MAX>::MIN;
        Generation(
            // SAFETY: the packed word retains a nonzero generation in its high half.
            unsafe { NonZero::<u32>::new_unchecked((self.raw.get() >> 32) as u32) },
        )
    }
}

const _: () = {
    assert!(mem::size_of::<Generation>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Generation>>() == mem::size_of::<Generation>());
    assert!(mem::size_of::<Parts>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Option<Parts>>() == mem::size_of::<Parts>());
    assert!(mem::size_of::<Handle>() == mem::size_of::<Parts>());
    assert!(mem::size_of::<Option<Handle>>() == mem::size_of::<Handle>());
};
