//! Serializable civil-time values without ambient clock authority.

use std::{borrow, marker, num::NonZero, time};

/// Exact duration arithmetic retaining normalized seconds and nanoseconds.
pub trait DurationExt: Sized {
    /// Computes floor(((N - 1) * self + sample) / N), for N = 2, 4 or 8.
    fn average<const N: u64>(self, sample: Self) -> Self;
}

impl DurationExt for time::Duration {
    fn average<const N: u64>(self, sample: Self) -> Self {
        let count = const {
            assert!(N == 2 || N == 4 || N == 8);
            NonZero::<u64>::MIN.saturating_add(N - 1)
        };
        let weight = const { N - 1 };
        let secs = weight * (self.as_secs() / count) + sample.as_secs() / count;
        let nanos = (weight * (self.as_secs() % count) + sample.as_secs() % count)
            * const { 1_000_000_000 / N }
            + (weight * u64::from(self.subsec_nanos()) + u64::from(sample.subsec_nanos())) / count;
        Self::new(secs + nanos / 1_000_000_000, (nanos % 1_000_000_000) as u32)
    }
}

/// A representation-preserving proof that a duration is non-zero.
/// APIs can retain it instead of rechecking the duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Positive(time::Duration);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zero;

impl Positive {
    pub const fn from_secs<const N: u64>() -> Self {
        const { assert!(N != 0) };
        Self(time::Duration::from_secs(N))
    }

    pub const fn from_millis<const N: u64>() -> Self {
        const { assert!(N != 0) };
        Self(time::Duration::from_millis(N))
    }

    pub const fn from_nanos<const N: u64>() -> Self {
        const { assert!(N != 0) };
        Self(time::Duration::from_nanos(N))
    }

    pub const fn try_new(duration: time::Duration) -> Result<Self, Zero> {
        if duration.is_zero() {
            Err(Zero)
        } else {
            Ok(Self(duration))
        }
    }

    pub const fn get(self) -> time::Duration {
        self.0
    }
}

impl borrow::Borrow<time::Duration> for Positive {
    fn borrow(&self) -> &time::Duration {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nanos;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Millis;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seconds;

/// Unit-branded ticks elapsed since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Unix<U>(u64, marker::PhantomData<fn(U) -> U>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Add {
    Scale {
        at: Unix<Nanos>,
        duration: time::Duration,
    },
    Range {
        at: Unix<Nanos>,
        duration: time::Duration,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    Scale {
        at: Unix<Nanos>,
        duration: time::Duration,
    },
    Range {
        at: Unix<Nanos>,
        duration: time::Duration,
    },
}

impl<U> Unix<U> {
    pub const ZERO: Self = Self::new(0);

    pub const fn new(ticks: u64) -> Self {
        Self(ticks, marker::PhantomData)
    }

    pub const fn ticks(&self) -> u64 {
        self.0
    }
}

impl Unix<Nanos> {
    pub fn try_add(self, duration: time::Duration) -> Result<Self, Add> {
        let Some(ticks) = Self::nanos(duration) else {
            return Err(Add::Scale { at: self, duration });
        };
        if self.0 > u64::MAX - ticks {
            return Err(Add::Range { at: self, duration });
        }
        Ok(Self::new(self.0 + ticks))
    }

    pub fn try_sub(self, duration: time::Duration) -> Result<Self, Sub> {
        let Some(ticks) = Self::nanos(duration) else {
            return Err(Sub::Scale { at: self, duration });
        };
        if ticks > self.0 {
            return Err(Sub::Range { at: self, duration });
        }
        Ok(Self::new(self.0 - ticks))
    }

    fn nanos(duration: time::Duration) -> Option<u64> {
        let nanos = duration.as_nanos();
        (nanos <= u64::MAX as u128).then_some(nanos as u64)
    }

    pub const fn floor_millis(self) -> Unix<Millis> {
        Unix::new(self.0 / 1_000_000)
    }

    pub const fn floor_secs(self) -> Unix<Seconds> {
        Unix::new(self.0 / 1_000_000_000)
    }
}

impl Unix<Millis> {
    pub const fn floor_secs(self) -> Unix<Seconds> {
        Unix::new(self.0 / 1_000)
    }
}

const _: () = {
    assert!(std::mem::size_of::<Positive>() == std::mem::size_of::<time::Duration>());
    assert!(std::mem::align_of::<Positive>() == std::mem::align_of::<time::Duration>());
    assert!(!std::mem::needs_drop::<Positive>());
};
const _: () = {
    assert!(std::mem::size_of::<Unix<Nanos>>() == std::mem::size_of::<u64>());
    assert!(std::mem::align_of::<Unix<Nanos>>() == std::mem::align_of::<u64>());
    assert!(!std::mem::needs_drop::<Unix<Nanos>>());
};
const _: () = {
    assert!(
        std::mem::size_of::<Add>() <= std::mem::size_of::<(Unix<Nanos>, time::Duration, u64)>()
    );
    assert!(
        std::mem::align_of::<Add>() <= std::mem::align_of::<(Unix<Nanos>, time::Duration, u64)>()
    );
    assert!(!std::mem::needs_drop::<Add>());
    assert!(std::mem::size_of::<Sub>() == std::mem::size_of::<Add>());
    assert!(std::mem::align_of::<Sub>() == std::mem::align_of::<Add>());
    assert!(!std::mem::needs_drop::<Sub>());
};
