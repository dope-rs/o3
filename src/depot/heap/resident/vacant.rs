use crate::depot::{heap, index::Position};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Vacant<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position = u32> {
    pub(in crate::depot::heap) entry: heap::Vacancy<&'a mut resident::Min<K, P, R, I>, K>,
}

use heap::resident;
impl<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position> resident::Vacant<'_, K, P, R, I> {
    pub fn key(&self) -> K {
        self.entry.value
    }
    pub fn resident(&self) -> &R {
        // SAFETY: this vacancy retains one key admitted to the owner's index
        // domain, and the resident table holds one value per such index.
        unsafe { heap::raw::resident(self.entry.owner, self.entry.value) }
    }
    pub fn resident_mut(&mut self) -> &mut R {
        // SAFETY: the key is in the owner's domain and the exclusive vacancy borrow
        // keeps this resident value unique.
        unsafe { heap::raw::resident_mut(self.entry.owner, self.entry.value) }
    }
    pub fn insert(self, priority: P) {
        self.entry.prepare(priority).store();
    }
}
