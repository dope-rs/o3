use std::marker;

mod seal;
pub(super) use seal::Seal;

pub trait Length: Seal + marker::Copy {
    const MAX: usize;
    const ZERO: Self;

    fn from_usize(value: usize) -> Self;
    fn into_usize(self) -> usize;

    /// # Safety
    /// The sum must fit this length representation and therefore `usize`.
    unsafe fn add(self, amount: usize) -> Self {
        Self::from_usize(self.into_usize() + amount)
    }
}

impl Length for u8 {
    const MAX: usize = u8::MAX as usize;
    const ZERO: Self = 0;

    fn from_usize(value: usize) -> Self {
        value as Self
    }

    fn into_usize(self) -> usize {
        self as usize
    }
}

impl Length for u16 {
    const MAX: usize = u16::MAX as usize;
    const ZERO: Self = 0;

    fn from_usize(value: usize) -> Self {
        value as Self
    }

    fn into_usize(self) -> usize {
        self as usize
    }
}

impl Length for u32 {
    const MAX: usize = u32::MAX as usize;
    const ZERO: Self = 0;

    fn from_usize(value: usize) -> Self {
        value as Self
    }

    fn into_usize(self) -> usize {
        self as usize
    }
}

impl Length for usize {
    const MAX: usize = usize::MAX;
    const ZERO: Self = 0;

    fn from_usize(value: usize) -> Self {
        value
    }

    fn into_usize(self) -> usize {
        self
    }
}
