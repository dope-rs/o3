use std::ops::{BitXorAssign, Shl, Shr};

use o3::base::num::bounded;

pub mod drops;

pub enum Tag {}

pub const CAPACITY: usize = 31;
pub const STEPS: usize = 100_000;

#[derive(Debug)]
pub struct Random<T, const A: u32, const B: u32, const C: u32>(T);

impl<T, const A: u32, const B: u32, const C: u32> Random<T, A, B, C>
where
    T: Copy + BitXorAssign + Shl<u32, Output = T> + Shr<u32, Output = T>,
{
    pub fn advance(&mut self) -> T {
        self.0 ^= self.0 << A;
        self.0 ^= self.0 >> B;
        self.0 ^= self.0 << C;
        self.0
    }
}

impl<const A: u32, const B: u32, const C: u32> Random<u32, A, B, C> {
    pub fn try_new(seed: u32) -> Result<Self, bounded::Overflow<{ u32::BITS - 1 }>> {
        bounded::U32::<0, { u32::BITS - 1 }>::try_usize(A.max(B).max(C) as usize)?;
        Ok(Self(seed))
    }
}

impl<const A: u32, const B: u32, const C: u32> Random<u64, A, B, C> {
    pub fn try_new(seed: u64) -> Result<Self, bounded::Overflow<{ u64::BITS - 1 }>> {
        bounded::U32::<0, { u64::BITS - 1 }>::try_usize(A.max(B).max(C) as usize)?;
        Ok(Self(seed))
    }
}

#[derive(Debug)]
pub struct Lcg<const STEP: u64 = 1>(u64);

impl<const STEP: u64> Lcg<STEP> {
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn advance(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(STEP);
        self.0
    }
}
