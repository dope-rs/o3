use core::num;

/// A `u32` proven to lie in the inclusive `MIN..=MAX` bounds.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U32<const MIN: u32, const MAX: u32>(u32);

impl<const MIN: u32, const MAX: u32> U32<MIN, MAX> {
    pub const fn new(value: u32) -> Option<Self> {
        if MIN <= MAX && value >= MIN && value <= MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn from_usize(value: usize) -> Option<Self> {
        if usize::BITS <= u32::BITS || value <= u32::MAX as usize {
            Self::new(value as u32)
        } else {
            None
        }
    }

    /// Clamps to the bounds, panicking if `MIN > MAX`.
    pub const fn clamp_from_usize(value: usize) -> Self {
        assert!(MIN <= MAX, "cannot clamp into an empty bounded range");
        if value < MIN as usize {
            Self(MIN)
        } else if value > MAX as usize {
            Self(MAX)
        } else {
            Self(value as u32)
        }
    }

    pub const fn checked_add(self, value: u32) -> Option<Self> {
        match self.0.checked_add(value) {
            Some(value) => Self::new(value),
            None => None,
        }
    }

    pub const fn checked_sub(self, value: u32) -> Option<Self> {
        match self.0.checked_sub(value) {
            Some(value) => Self::new(value),
            None => None,
        }
    }

    pub const fn checked_add_usize(self, value: usize) -> Option<Self> {
        match (self.0 as usize).checked_add(value) {
            Some(value) => Self::from_usize(value),
            None => None,
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn into_usize(self) -> usize {
        self.0 as usize
    }
}

impl<const MIN: u32, const MAX: u32> From<U32<MIN, MAX>> for u32 {
    fn from(value: U32<MIN, MAX>) -> Self {
        value.get()
    }
}

/// A `u64` proven to lie in the inclusive `MIN..=MAX` range.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U64<const MIN: u64, const MAX: u64>(u64);

impl<const MIN: u64, const MAX: u64> U64<MIN, MAX> {
    pub const fn new(value: u64) -> Option<Self> {
        if MIN <= MAX && value >= MIN && value <= MAX {
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

    /// Clamps to the bounds, panicking if `MIN > MAX`.
    pub const fn clamp_from_usize(value: usize) -> Self {
        assert!(MIN <= MAX, "cannot clamp into an empty bounded range");
        if value < MIN as usize {
            Self(MIN)
        } else if value > MAX as usize {
            Self(MAX)
        } else {
            Self(value as u64)
        }
    }

    pub const fn checked_add(self, value: u64) -> Option<Self> {
        match self.0.checked_add(value) {
            Some(value) => Self::new(value),
            None => None,
        }
    }

    pub const fn checked_sub(self, value: u64) -> Option<Self> {
        match self.0.checked_sub(value) {
            Some(value) => Self::new(value),
            None => None,
        }
    }

    pub const fn checked_add_usize(self, value: usize) -> Option<Self> {
        match (self.0 as usize).checked_add(value) {
            Some(value) => Self::from_usize(value),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn into_usize(self) -> usize {
        self.0 as usize
    }
}

impl<const MIN: u64, const MAX: u64> From<U64<MIN, MAX>> for u64 {
    fn from(value: U64<MIN, MAX>) -> Self {
        value.get()
    }
}

/// A nonzero `u64` proven to lie in the inclusive `MIN..=MAX` range.
///
/// Its representation retains the `NonZeroU64` null niche.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonZeroU64<const MIN: u64, const MAX: u64>(num::NonZeroU64);

impl<const MIN: u64, const MAX: u64> NonZeroU64<MIN, MAX> {
    pub const fn new(value: u64) -> Option<Self> {
        match num::NonZeroU64::new(value) {
            Some(value) if MIN <= MAX && value.get() >= MIN && value.get() <= MAX => {
                Some(Self(value))
            }
            Some(_) | None => None,
        }
    }

    pub const fn from_usize(value: usize) -> Option<Self> {
        if usize::BITS <= u64::BITS || value <= u64::MAX as usize {
            Self::new(value as u64)
        } else {
            None
        }
    }

    pub const fn checked_add(self, value: u64) -> Option<Self> {
        match self.0.get().checked_add(value) {
            Some(value) => Self::new(value),
            None => None,
        }
    }

    pub const fn checked_add_usize(self, value: usize) -> Option<Self> {
        match (self.0.get() as usize).checked_add(value) {
            Some(value) => Self::from_usize(value),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn into_usize(self) -> usize {
        self.0.get() as usize
    }
}

impl<const MIN: u64, const MAX: u64> From<NonZeroU64<MIN, MAX>> for u64 {
    fn from(value: NonZeroU64<MIN, MAX>) -> Self {
        value.get()
    }
}

impl<const MIN: u64, const MAX: u64> From<NonZeroU64<MIN, MAX>> for num::NonZeroU64 {
    fn from(value: NonZeroU64<MIN, MAX>) -> Self {
        value.0
    }
}
