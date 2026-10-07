use std::{marker, mem, num::NonZero};

pub use error::Error;
pub use stale::Stale;

use crate::depot::{arena::slab::external, capacity, index::Stored};

pub mod cycle;
mod error;
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

#[derive(Debug)]
#[repr(transparent)]
pub struct Handle<Tag = (), const MAX: u32 = { u32::MAX }> {
    parts: Parts<MAX>,
    marker: marker::PhantomData<*mut Tag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Parts<const MAX: u32 = { u32::MAX }> {
    raw: NonZero<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Generation<const MAX: u32 = { u32::MAX }>(NonZero<u32>, Limit<MAX>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limit<const MAX: u32>(marker::PhantomData<()>);

impl<const MAX: u32> Limit<MAX> {
    pub(crate) const NEW: Self = {
        assert!(MAX != 0, "generation limit must be nonzero");
        Self(marker::PhantomData)
    };

    pub(crate) const fn min(self) -> Generation<MAX> {
        Generation(NonZero::<u32>::MIN, self)
    }

    const fn contains(raw: u32) -> bool {
        raw != 0 && raw <= MAX
    }

    pub(crate) const fn check(self, raw: u32) -> Option<Generation<MAX>> {
        if !Self::contains(raw) {
            return None;
        }
        // SAFETY: the exact generation bounds above exclude zero.
        Some(Generation(
            unsafe { NonZero::<u32>::new_unchecked(raw) },
            self,
        ))
    }
}

impl<const MAX: u32> Generation<MAX> {
    pub const MIN: Self = Limit::NEW.min();

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// The retained generation as its nonzero wire value, without a second check.
    pub const fn nonzero(self) -> NonZero<u32> {
        self.0
    }

    #[must_use]
    pub const fn checked_add(self, value: u32) -> Option<Self> {
        let (raw, overflow) = self.0.get().overflowing_add(value);
        if overflow { None } else { self.1.check(raw) }
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
        if !Limit::<MAX>::contains(epoch) {
            return None;
        }
        // SAFETY: the shared generation predicate above excludes zero.
        let epoch = Generation(unsafe { NonZero::<u32>::new_unchecked(epoch) }, Limit::NEW);
        Some(Self::new(raw.get() as u32, epoch))
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
        Generation(
            // SAFETY: the packed word retains a nonzero generation in its high half.
            unsafe { NonZero::<u32>::new_unchecked((self.raw.get() >> 32) as u32) },
            Limit::NEW,
        )
    }
}

impl<Tag, const MAX: u32> Handle<Tag, MAX> {
    pub(crate) const fn new(index: u32, epoch: Generation<MAX>) -> Self {
        Self::from_parts(Parts::new(index, epoch))
    }

    #[doc(hidden)]
    pub const fn from_parts(parts: Parts<MAX>) -> Self {
        Self {
            parts,
            marker: marker::PhantomData,
        }
    }

    pub(in crate::depot) fn from_stored(raw: Stored<Self>) -> Self {
        // SAFETY: Stored retains a complete identity emitted by this Handle domain.
        let raw = unsafe { NonZero::<u64>::new_unchecked(raw.into_raw()) };
        Self::from_parts(Parts { raw })
    }

    pub const fn index(self) -> u32 {
        self.parts.index()
    }

    pub const fn generation(self) -> Generation<MAX> {
        self.parts.generation()
    }

    pub const fn parts(self) -> Parts<MAX> {
        self.parts
    }
}

impl<Tag, const MAX: u32> From<Handle<Tag, MAX>> for Parts<MAX> {
    fn from(key: Handle<Tag, MAX>) -> Self {
        key.parts
    }
}

impl<Tag, const MAX: u32> Clone for Handle<Tag, MAX> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, const MAX: u32> Copy for Handle<Tag, MAX> {}

impl<Tag, const MAX: u32> PartialEq for Handle<Tag, MAX> {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl<Tag, const MAX: u32> Eq for Handle<Tag, MAX> {}

const _: () = {
    assert!(mem::size_of::<Limit<{ u32::MAX }>>() == 0);
    assert!(mem::size_of::<Generation>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Generation>>() == mem::size_of::<Generation>());
    assert!(mem::size_of::<Parts>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Option<Parts>>() == mem::size_of::<Parts>());
    assert!(mem::size_of::<Handle>() == mem::size_of::<Parts>());
    assert!(mem::size_of::<Option<Handle>>() == mem::size_of::<Handle>());
};
