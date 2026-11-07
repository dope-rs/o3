use std::num::NonZero;

use o3::depot::{self, heap::indexed};

pub mod observed;
pub mod resident;
pub mod split;

pub fn into_vacant<'a, P: Ord>(entry: indexed::Entry<'a, P>) -> Option<indexed::Vacant<'a, P>> {
    match entry {
        indexed::Entry::Vacant(entry) => Some(entry),
        indexed::Entry::Occupied(_) => None,
    }
}

pub fn occupied<'a, P: Ord>(entry: indexed::Entry<'a, P>) -> Option<indexed::Occupied<'a, P>> {
    match entry {
        indexed::Entry::Occupied(entry) => Some(entry),
        indexed::Entry::Vacant(_) => None,
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
