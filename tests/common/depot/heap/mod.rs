use std::num::NonZero;

use o3::depot::{self, heap::indexed};

pub mod observed;
pub mod resident;
pub mod split;

pub trait EntryExt<'a, P: Ord> {
    fn into_vacant(self) -> Option<indexed::Vacant<'a, P>>;
    fn occupied(self) -> Option<indexed::Occupied<'a, P>>;
}

impl<'a, P: Ord> EntryExt<'a, P> for indexed::Entry<'a, P> {
    fn into_vacant(self) -> Option<indexed::Vacant<'a, P>> {
        match self {
            Self::Vacant(entry) => Some(entry),
            Self::Occupied(_) => None,
        }
    }

    fn occupied(self) -> Option<indexed::Occupied<'a, P>> {
        match self {
            Self::Occupied(entry) => Some(entry),
            Self::Vacant(_) => None,
        }
    }
}

#[repr(align(64))]
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct Aligned(pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address {
    pub index: u8,
    pub generation: NonZero<u8>,
}

unsafe impl depot::raw::Address for Address {
    fn index(self) -> usize {
        self.index as usize
    }
}
