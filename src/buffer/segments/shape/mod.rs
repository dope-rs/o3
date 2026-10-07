use crate::buffer;
pub(in crate::buffer::segments) mod nonempty;
pub(in crate::buffer::segments) mod owners;

use crate::buffer::segments::{Count, Item, Many, Owners};

/// Physical owner cardinality preserved through consuming classification.
#[derive(Debug)]
pub enum Shape<T: buffer::raw::Segment> {
    Empty,
    One(Item<T>),
    Many(Many<T>),
}

impl<T: buffer::raw::Segment> Shape<T> {
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::One(owner) => owner.as_ref().as_ref().len(),
            Self::Many(owners) => owners.len().get(),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn count(&self) -> Count {
        match self {
            Self::Empty => Count::EMPTY,
            Self::One(owner) => owner.count(),
            Self::Many(owners) => buffer::raw::Total::count(&owners),
        }
    }

    pub fn contiguous(&self) -> Option<&[u8]> {
        match self {
            Self::Empty => Some(&[]),
            Self::One(owner) => Some(owner.as_ref().as_ref()),
            Self::Many(_) => None,
        }
    }

    pub fn owners(&self) -> Owners<'_, T> {
        Owners::new(self)
    }

    pub fn segments(&self) -> impl ExactSizeIterator<Item = &[u8]> + iter::FusedIterator {
        self.owners().map(AsRef::as_ref)
    }
}
use std::iter;
