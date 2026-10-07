use std::num::NonZero;

pub mod raw;

/// A `u32` proven to lie in the inclusive `MIN..=MAX` bounds.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U32<const MIN: u32, const MAX: u32>(u32);

impl<const MIN: u32, const MAX: u32> U32<MIN, MAX> {
    const NONEMPTY: () = assert!(MIN <= MAX, "bounded U32 clamp requires MIN <= MAX");

    pub const fn new(value: u32) -> Option<Self> {
        if value >= MIN && value <= MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Constructs the bound from a compile-time value within its range.
    pub const fn exact<const VALUE: u32>() -> Self {
        const { assert!(MIN <= VALUE && VALUE <= MAX, "bounds") };
        Self(VALUE)
    }

    pub const fn from_usize(value: usize) -> Option<Self> {
        if usize::BITS <= u32::BITS || value <= u32::MAX as usize {
            Self::new(value as u32)
        } else {
            None
        }
    }

    /// Clamps to the inclusive bounds.
    pub const fn clamp(value: usize) -> Self {
        let () = Self::NONEMPTY;
        if value < MIN as usize {
            Self(MIN)
        } else if value > MAX as usize {
            Self(MAX)
        } else {
            Self(value as u32)
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// Retains the range of the difference between two admitted values.
    pub const fn abs_diff(self, other: Self) -> U32<0, MAX> {
        U32(self.0.abs_diff(other.0))
    }

    pub const fn into_usize(self) -> usize {
        self.get() as usize
    }

    /// Widens the retained range without checking its value again.
    pub const fn widen<const LOW: u32, const HIGH: u32>(self) -> U32<LOW, HIGH> {
        const { assert!(LOW <= MIN && MAX <= HIGH, "widen") };
        U32(self.0)
    }
}

impl<const MAX: u32> U32<0, MAX> {
    /// Selects a count no greater than the retained limit.
    pub const fn prefix(value: usize, limit: Self) -> Self {
        if value > limit.0 as usize {
            limit
        } else {
            Self(value as u32)
        }
    }

    pub const fn try_usize(value: usize) -> Result<Self, Overflow<MAX>> {
        match NonZero::<usize>::new(value.saturating_sub(MAX as usize)) {
            Some(excess) => Err(Overflow::new(excess)),
            None => Ok(Self(value as u32)),
        }
    }
}

/// A `u32` proven to lie in the inclusive `MIN..=MAX` range of a nonzero value.
///
/// The field is nonzero, so `Option` keeps the niche that a plain bounded value
/// loses: `Option<NonZeroU32<MIN, MAX>>` stays one word wide.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonZeroU32<const MIN: u32, const MAX: u32>(NonZero<u32>);

impl<const MIN: u32, const MAX: u32> NonZeroU32<MIN, MAX> {
    const RANGE: () = assert!(
        1 <= MIN && MIN <= MAX,
        "nonzero bound requires 1 <= MIN <= MAX"
    );

    pub const fn new(value: u32) -> Option<Self> {
        let () = Self::RANGE;
        match NonZero::new(value) {
            Some(value) if value.get() >= MIN && value.get() <= MAX => Some(Self(value)),
            _ => None,
        }
    }

    /// Constructs the bound from a compile-time value within its range.
    pub const fn exact<const VALUE: u32>() -> Self {
        const { assert!(1 <= MIN && MIN <= VALUE && VALUE <= MAX, "nonzero bounds") };
        Self(NonZero::<u32>::MIN.saturating_add(VALUE - 1))
    }

    pub const fn from_usize(value: usize) -> Option<Self> {
        if usize::BITS <= u32::BITS || value <= u32::MAX as usize {
            Self::new(value as u32)
        } else {
            None
        }
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    pub const fn nonzero(self) -> NonZero<u32> {
        self.0
    }

    /// Widens this bound to the same range without a plain bounded value.
    pub const fn into_u32(self) -> U32<MIN, MAX> {
        U32(self.get())
    }

    /// Converts a one-based position without rechecking its retained range.
    pub const fn into_index<const END: u32>(self) -> U32<0, END> {
        const { assert!(MAX != 0 && MAX - 1 <= END, "bounds") };
        U32(self.get() - 1)
    }

    pub const fn into_usize(self) -> usize {
        self.get() as usize
    }
}

impl U32<0, { u32::MAX - 1 }> {
    /// Converts a nonzero one-based position to its zero-based index.
    pub const fn from_one(position: NonZero<u32>) -> Self {
        Self(position.get() - 1)
    }
}

/// An extent beyond the inclusive `0..=MAX` range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Overflow<const MAX: u32> {
    excess: NonZero<usize>,
}

impl<const MAX: u32> Overflow<MAX> {
    /// `excess` is the difference between an admitted `usize` and `MAX`.
    pub(crate) const fn new(excess: NonZero<usize>) -> Self {
        Self { excess }
    }

    pub const fn actual(self) -> usize {
        MAX as usize + self.excess.get()
    }
}

/// A `u64` proven to lie in the inclusive `MIN..=MAX` range.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U64<const MIN: u64, const MAX: u64>(u64);

impl<const MIN: u64, const MAX: u64> U64<MIN, MAX> {
    const NONEMPTY: () = assert!(MIN <= MAX, "bounded U64 clamp requires MIN <= MAX");

    pub const fn new(value: u64) -> Option<Self> {
        if value >= MIN && value <= MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn from_usize(value: usize) -> Option<Self> {
        if usize::BITS <= u64::BITS || value <= u64::MAX as usize {
            Self::new(value as u64)
        } else {
            None
        }
    }

    /// Clamps to the inclusive bounds.
    pub const fn clamp(value: u64) -> Self {
        let () = Self::NONEMPTY;
        if value < MIN {
            Self(MIN)
        } else if value > MAX {
            Self(MAX)
        } else {
            Self(value)
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<const MAX: u64> U64<0, MAX> {
    /// Subtracts while retaining this value's admitted upper bound.
    pub const fn checked_sub(self, value: u64) -> Option<Self> {
        match self.0.checked_sub(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Extracts the bits selected by `MAX`, retaining their numeric upper bound.
    pub const fn mask(value: u64) -> Self {
        Self(value & MAX)
    }
}

/// A count in `0..=MAX`, retained through checked additions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Usize<const MAX: usize>(usize);

/// A nonzero `usize` retained within the inclusive `MIN..=MAX` bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NonZeroUsize<const MIN: usize, const MAX: usize>(NonZero<usize>);

impl<const MIN: usize, const MAX: usize> NonZeroUsize<MIN, MAX> {
    const RANGE: () = assert!(
        1 <= MIN && MIN <= MAX,
        "nonzero bound requires 1 <= MIN <= MAX"
    );

    pub const fn new(value: usize) -> Option<Self> {
        let () = Self::RANGE;
        match NonZero::new(value) {
            Some(value) if value.get() >= MIN && value.get() <= MAX => Some(Self(value)),
            _ => None,
        }
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Retains the admitted count without checking its upper bound again.
    pub const fn into_count(self) -> Usize<MAX> {
        Usize(self.get())
    }

    /// Returns the predecessor proved by this nonzero count.
    pub const fn prior(self) -> Usize<MAX> {
        Usize(self.get() - 1)
    }
}

impl<const MAX: usize> NonZeroUsize<1, MAX> {
    /// Admits one successor against this retained capacity.
    pub const fn after(self, value: Usize<MAX>) -> Option<Self> {
        let value = value.get();
        if value < self.get() {
            Some(Self(NonZero::<usize>::MIN.saturating_add(value)))
        } else {
            None
        }
    }
}

/// The exact addition that exceeded its remaining bounded range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub value: usize,
    pub incoming: NonZero<usize>,
    pub maximum: usize,
}

impl<const MAX: usize> Usize<MAX> {
    pub const ZERO: Self = Self(0);

    pub const fn get(self) -> usize {
        self.0
    }

    /// Classifies zero while retaining the admitted upper bound.
    pub const fn nonzero(self) -> Option<NonZeroUsize<1, MAX>> {
        match NonZero::new(self.0) {
            Some(value) => Some(NonZeroUsize(value)),
            None => None,
        }
    }

    pub const fn try_add(self, incoming: usize) -> Result<Self, Error> {
        if incoming > MAX - self.0 {
            return Err(Error {
                value: self.0,
                incoming: NonZero::<usize>::MIN.saturating_add(incoming - 1),
                maximum: MAX,
            });
        }
        Ok(Self(self.0 + incoming))
    }

    /// Narrows a proved range without validating its value again.
    pub const fn into_u32<const END: u32>(self) -> U32<0, END> {
        const { assert!(MAX <= END as usize) };
        U32(self.0 as u32)
    }
}

const _: () = {
    assert!(size_of::<NonZeroU32<1, 16>>() == size_of::<u32>());
    assert!(size_of::<Option<NonZeroU32<1, 16>>>() == size_of::<u32>());
    assert!(size_of::<NonZeroUsize<1, 16>>() == size_of::<usize>());
    assert!(size_of::<Option<NonZeroUsize<1, 16>>>() == size_of::<usize>());
    assert!(size_of::<Overflow<{ i32::MAX as u32 }>>() == size_of::<usize>());
    assert!(size_of::<Usize<16_384>>() == size_of::<usize>());
    assert!(size_of::<Error>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Error>>() == size_of::<Error>());
};
