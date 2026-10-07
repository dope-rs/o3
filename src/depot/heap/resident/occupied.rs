use crate::depot::{heap, index::Position};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Occupied<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position = u32> {
    pub(in crate::depot::heap) entry: heap::Entry<&'a mut resident::Min<K, P, R, I>, K>,
}

use heap::resident;
impl<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position>
    resident::Occupied<'a, K, P, R, I>
{
    pub fn key(&self) -> K {
        self.entry.next
    }
    pub fn priority(&self) -> &P {
        &self.entry.value().priority
    }
    pub fn resident(&self) -> &R {
        // SAFETY: this occupied entry retains one key admitted to the owner's
        // index domain, and the resident table holds one value per such index.
        unsafe { heap::raw::resident(self.entry.owner, self.entry.next) }
    }
    pub fn resident_mut(&mut self) -> &mut R {
        // SAFETY: the key is in the owner's domain and the exclusive entry borrow
        // keeps this resident value unique.
        unsafe { heap::raw::resident_mut(self.entry.owner, self.entry.next) }
    }
    pub fn set(self, priority: P) {
        heap::raw::Hole::update(self.entry.into_parts().0.prepare(priority));
    }
    pub fn set_min(self, priority: P) -> heap::Change {
        self.entry.into_parts().0.set_min_slot(priority)
    }
    pub fn remove(self) -> resident::Removed<'a, K, P, R, I> {
        resident::Removed {
            entry: heap::raw::Hole::remove(self.entry.into_parts().0),
        }
    }
    pub fn retire(self) {
        heap::raw::Hole::retire(self.entry.into_parts().0);
    }
    pub fn retire_min(self) -> heap::Change {
        self.entry.into_parts().0.retire_min()
    }
}
