use crate::{
    alloc,
    depot::{capacity, heap},
};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Vacant<'a, P: Ord, C: alloc::raw::Extent = capacity::Count> {
    pub(in crate::depot::heap) entry: heap::Vacancy<&'a mut indexed::Min<P, C>, heap::Direct>,
}

use heap::indexed;
impl<'a, P: Ord, C: alloc::raw::Extent> indexed::Vacant<'a, P, C> {
    pub fn set(self, value: P) {
        self.entry.prepare(value).store();
    }
    pub fn insert(self, value: P) -> indexed::Occupied<'a, P, C> {
        indexed::Occupied {
            entry: heap::raw::Hole::append(self.entry.prepare(value)).finish(),
        }
    }
}
