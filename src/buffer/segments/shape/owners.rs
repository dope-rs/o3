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

impl<'a, T: buffer::raw::Segment> Owners<'a, T> {
    pub(in crate::buffer::segments) fn new(shape: &'a Shape<T>) -> Self {
        match shape {
            Shape::Empty => Self::Empty,
            Shape::One(owner) => Self::One(owner),
            Shape::Many(owners) => Self::Many(owners.as_chain().segments()),
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
