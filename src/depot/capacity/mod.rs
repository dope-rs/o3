use std::{mem, num::NonZero};

pub use domain::Domain;
pub use key::Key;

use crate::{alloc, depot, depot::capacity};

mod domain;
mod key;

/// An exact collection capacity retained in its owning type until allocation.
pub trait Extent: Copy {
    #[doc(hidden)]
    fn get(self) -> usize;
}

/// A count representable by every u32-indexed collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Count(u32);

/// A requested count outside the u32 index domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    requested: usize,
}

/// The exact demand rejected by a bounded store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortage {
    pub requested: NonZero<usize>,
    pub available: usize,
}

/// Why a bounded collection could not reserve capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    Full,
    Allocation(alloc::Error),
}

impl Count {
    pub const EMPTY: Self = Self(0);
    pub const MAX: Self = Self(u32::MAX);

    #[must_use]
    pub const fn new(count: u32) -> Self {
        Self(count)
    }

    /// Admits a count whose last represented index fits the supplied domain.
    pub(in crate::depot) const fn try_index<E: Copy>(
        count: usize,
        maximum: usize,
        error: E,
    ) -> Result<Self, E> {
        if count > u32::MAX as usize || (count != 0 && count - 1 > maximum) {
            return Err(error);
        }
        Ok(Self(count as u32))
    }

    pub(in crate::depot) const fn try_count<E: Copy>(count: usize, error: E) -> Result<Self, E> {
        Self::try_index(count, u32::MAX as usize, error)
    }

    #[inline]
    pub fn try_nonzero(count: NonZero<usize>) -> Result<NonZero<u32>, Error> {
        Self::try_narrow(count, Error::new(count.get()))
    }

    pub(in crate::depot) fn try_narrow<E: Copy>(
        count: NonZero<usize>,
        error: E,
    ) -> Result<NonZero<u32>, E> {
        NonZero::<u32>::try_from(count).map_err(|_| error)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn nonzero(self) -> Option<NonZero<u32>> {
        NonZero::new(self.0)
    }
}

impl TryFrom<usize> for Count {
    type Error = Error;

    fn try_from(count: usize) -> Result<Self, Self::Error> {
        Self::try_count(count, Error::new(count))
    }
}

impl Extent for Count {
    fn get(self) -> usize {
        self.0 as usize
    }
}

impl Extent for usize {
    fn get(self) -> usize {
        self
    }
}

impl Error {
    pub(crate) const fn new(count: usize) -> Self {
        Self { requested: count }
    }

    #[must_use]
    pub const fn requested(self) -> usize {
        self.requested
    }
}

const _: () = assert!(mem::size_of::<Reason>() == 1);

/// A nonzero count bounded by `MAX` and the u32 index domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Limit<const MAX: usize>(NonZero<u32>);

impl<const MAX: usize> Limit<MAX> {
    pub const MAX: usize = if MAX > u32::MAX as usize {
        u32::MAX as usize
    } else {
        MAX
    };

    pub const fn new(value: usize) -> Option<Self> {
        if value == 0 || value > Self::MAX {
            return None;
        }
        Some(Self(NonZero::<u32>::MIN.saturating_add(value as u32 - 1)))
    }

    pub const fn fixed<const N: usize>() -> Self {
        const {
            assert!(N != 0 && N <= Self::MAX);
        }
        Self(NonZero::<u32>::MIN.saturating_add(N as u32 - 1))
    }

    pub const fn get(self) -> usize {
        self.0.get() as usize
    }

    pub const fn count(self) -> NonZero<u32> {
        self.0
    }
}

// SAFETY: every copy retains the same immutable bounded count.
unsafe impl<const MAX: usize> alloc::raw::Extent for Limit<MAX> {
    fn len(&self) -> usize {
        self.0.get() as usize
    }
}

// SAFETY: every copy retains the same immutable represented count.
unsafe impl alloc::raw::Extent for capacity::Count {
    fn len(&self) -> usize {
        self.0 as usize
    }
}

// SAFETY: the original represented count is retained unchanged.
unsafe impl depot::raw::Count for capacity::Count {
    fn count(self) -> capacity::Count {
        self
    }
}

// SAFETY: every copy retains the same nonzero u32 count.
unsafe impl depot::raw::Count for NonZero<u32> {
    const MIN: u32 = 1;

    fn count(self) -> capacity::Count {
        capacity::Count::new(self.get())
    }
}

// SAFETY: construction retains a nonzero count bounded by MAX and the u32 domain.
unsafe impl<const MAX: usize> depot::raw::Count for Limit<MAX> {
    const MIN: u32 = 1;
    const MAX: u32 = Limit::<MAX>::MAX as u32;

    fn count(self) -> capacity::Count {
        capacity::Count::new(self.0.get())
    }
}

// SAFETY: the represented count is exact and `0..count` yields every index once.
unsafe impl alloc::raw::TailExtent for capacity::Count {
    type Index = u32;

    fn indices(self) -> impl Iterator<Item = Self::Index> {
        0..self.raw()
    }
}

const _: () = {
    assert!(mem::size_of::<Limit<65536>>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Limit<65536>>>() == mem::size_of::<u32>());
};

/// The retained (owner, key) pair names a physical slot in that owner's immutable extent.
/// Initialization and membership are not implied.
pub enum Physical {}

impl crate::depot::Predicate for Physical {}
