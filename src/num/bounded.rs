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

    pub const fn get(self) -> u32 {
        self.0
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

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<const MIN: u64, const MAX: u64> From<U64<MIN, MAX>> for u64 {
    fn from(value: U64<MIN, MAX>) -> Self {
        value.get()
    }
}
