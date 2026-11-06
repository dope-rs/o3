use std::iter;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Item, Shape},
    },
};

/// Borrowed owners of one exact physical shape.
pub enum Owners<'a, T> {
    Empty,
    One(&'a Item<T>),
    Many(segments::Iter<'a, T>),
}

impl<T: buffer::raw::Segment> Shape<T> {
    pub fn owners(&self) -> Owners<'_, T> {
        match self {
            Self::Empty => Owners::Empty,
            Self::One(owner) => Owners::One(owner),
            Self::Many(owners) => Owners::Many(owners.as_chain().segments()),
        }
    }
}

impl<'a, T: 'a> Iterator for Owners<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::One(owner) => {
                let owner = *owner;
                *self = Self::Empty;
                Some(owner.as_ref())
            }
            Self::Many(owners) => owners.next(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = match self {
            Self::Empty => 0,
            Self::One(_) => 1,
            Self::Many(owners) => owners.len(),
        };
        (left, Some(left))
    }
}
impl<T> ExactSizeIterator for Owners<'_, T> {}
impl<T> iter::FusedIterator for Owners<'_, T> {}

const _: () = {
    assert!(size_of::<Owners<'_, ()>>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<Owners<'_, ()>>>() == size_of::<Owners<'_, ()>>());
};
