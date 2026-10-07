use crate::depot::{heap, index::Position};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Conflict<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position = u32> {
    pub(in crate::depot::heap) entry: heap::Entry<&'a mut resident::Min<K, P, R, I>, K>,
}

use heap::resident;
impl<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position>
    resident::Conflict<'a, K, P, R, I>
{
    pub fn requested(&self) -> K {
        self.entry.next
    }
    pub fn current(&self) -> K {
        self.entry.value().identity
    }
    pub fn resident_mut(&mut self) -> &mut R {
        // SAFETY: the conflicting replacement is the requested key admitted to the
        // owner's index domain, and the exclusive borrow keeps this resident unique.
        unsafe { heap::raw::resident_mut(self.entry.owner, self.entry.next) }
    }
    pub fn set(self, priority: P) {
        heap::raw::Hole::prepare(self.entry.prepare_key(priority));
    }
}
