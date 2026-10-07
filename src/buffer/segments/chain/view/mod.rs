use crate::depot::arena::linked::slots;
pub(in crate::buffer::segments) mod cursor;
pub(in crate::buffer::segments) mod iter;

use crate::buffer::{segments, segments::handles::Descriptor};

pub(in crate::buffer::segments) enum State<'a, T> {
    Empty,
    Single(slots::Index, &'a segments::Item<T>),
    Multi(&'a Descriptor),
}

/// The live topology and byte extent borrowed from one nonempty owner.
pub(in crate::buffer::segments) enum Occupied<'a, T> {
    Single(slots::Index, &'a segments::Item<T>),
    Multi(&'a Descriptor),
}
