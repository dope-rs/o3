use std::ops::{BitAnd, BitOr, Not, Shl};

/// Primitive widths supported by bitmap words and hierarchy shapes.
pub trait Width:
    crate::depot::raw::Word
    + Sealed
    + BitAnd<Output = Self>
    + BitOr<Output = Self>
    + Not<Output = Self>
    + Shl<usize, Output = Self>
{
    const BITS: u32 = Self::MAX.get().ilog2() + 1;
}

impl Width for usize {}
impl Width for u32 {}

mod sealed;
pub(in crate::depot::index::bitmap) use sealed::Sealed;
