use crate::depot::{capacity, heap, index::Position};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Vacant<'a, K: crate::depot::raw::Address, P: Ord, I: Position = u32, B = ()> {
    pub(in crate::depot::heap) entry: heap::Vacancy<&'a mut keyed::Min<K, P, I, B>, K>,
}

use heap::keyed;
impl<K: crate::depot::raw::Address, P: Ord, I: Position, B> keyed::Vacant<'_, K, P, I, B> {
    pub fn insert(self, priority: P) {
        self.entry.prepare(priority).store();
    }
}

impl<'a, 'h, K: crate::depot::raw::Address, P: Ord, I: Position>
    keyed::Vacant<'a, K, P, I, capacity::Domain<'h>>
{
    /// Consumes the exact absent identity, spare capacity, and retained heap owner.
    pub fn from_live<C>(live: keyed::Admission<'a, 'h, K, P, I, C, Self>) -> (Self, C) {
        let ((heap, context), key) = live.into_inner();
        // SAFETY: this source plane retains the bounded absent identity and spare slot.
        let entry = unsafe { heap::Vacancy::new(heap, key.into_inner()) };
        (Self { entry }, context)
    }
}
