use crate::depot::{capacity, heap, index::Position};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Occupied<'a, K: crate::depot::raw::Address, P: Ord, I: Position = u32, B = ()> {
    pub(in crate::depot::heap) entry: heap::Entry<&'a mut keyed::Min<K, P, I, B>, K>,
}

use heap::keyed;
impl<K: crate::depot::raw::Address, P: Ord, I: Position, B> keyed::Occupied<'_, K, P, I, B> {
    pub fn priority(&self) -> &P {
        &self.entry.value().priority
    }
    pub fn set(self, priority: P) {
        heap::raw::Hole::update(self.entry.into_parts().0.prepare(priority));
    }
    pub fn retire_min(self) -> heap::Change {
        self.entry.into_parts().0.retire_min()
    }
    pub fn remove(self) -> P {
        heap::raw::Hole::take(self.entry.into_parts().0).priority
    }
}

impl<K: crate::depot::raw::Address, P: Ord, I: Position> keyed::Occupied<'_, K, P, I> {
    pub fn key(&self) -> K {
        self.entry.next
    }
}

impl<'a, K: crate::depot::raw::Address, P: Ord, I: Position>
    keyed::Occupied<'_, K, P, I, capacity::Domain<'a>>
{
    pub fn key(&self) -> capacity::Key<'a, K> {
        // SAFETY: this retained entry was admitted by its exact domain owner.
        capacity::Key::new(unsafe { crate::depot::raw::Proof::new(self.entry.next) })
    }
}

impl<'a, 'h, K: crate::depot::raw::Address, P: Ord, I: Position>
    keyed::Occupied<'a, K, P, I, capacity::Domain<'h>>
{
    /// Consumes one exact occupied membership and its retained heap owner.
    pub fn from_live<C>(live: keyed::Admission<'a, 'h, K, P, I, C, Self>) -> (Self, C) {
        let ((heap, context), key) = live.into_inner();
        let key = key.into_inner();
        // SAFETY: the occupied plane retains this complete identity at its live offset.
        let position = unsafe { *heap.storage.tail().get_unchecked(key.index()) };
        // SAFETY: the admission retains the exclusive heap and this key's exact
        // initialized offset, preserving its ordering domain and reverse map.
        let entry = unsafe { heap::Entry::new(heap, position.offset(), key) };
        (Self { entry }, context)
    }
}
