use std::num::NonZero;

use crate::{base::num::bounded, buffer::bytes::bounded::MAX};

/// Byte extent beyond the signed 32-bit domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Overflow {
    extent: bounded::Overflow<{ MAX as u32 }>,
}

impl Overflow {
    pub(crate) const fn new(excess: NonZero<usize>) -> Self {
        Self::from_num(bounded::Overflow::new(excess))
    }

    pub const fn from_num(extent: bounded::Overflow<{ MAX as u32 }>) -> Self {
        Self { extent }
    }

    pub const fn actual(self) -> usize {
        self.extent.actual()
    }
}

const _: () = assert!(size_of::<Overflow>() == size_of::<usize>());
