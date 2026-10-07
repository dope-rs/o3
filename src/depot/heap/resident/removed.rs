use crate::depot::{heap, index::Position};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Removed<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position = u32> {
    pub(in crate::depot::heap) entry:
        heap::Vacancy<&'a mut resident::Min<K, P, R, I>, heap::Stored<K, P>>,
}

use heap::resident;
impl<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position>
    resident::Removed<'_, K, P, R, I>
{
    pub fn key(&self) -> K {
        self.entry.value.identity
    }
    pub const fn priority(&self) -> &P {
        &self.entry.value.priority
    }
    pub fn resident(&self) -> &R {
        // SAFETY: a removed entry retains a key admitted to the owner's index
        // domain, and removal keeps the resident table one value per index.
        unsafe { heap::raw::resident(self.entry.owner, self.entry.value.identity) }
    }
    pub fn resident_mut(&mut self) -> &mut R {
        // SAFETY: the key is in the owner's domain and the exclusive removed borrow
        // keeps this resident value unique.
        unsafe { heap::raw::resident_mut(self.entry.owner, self.entry.value.identity) }
    }
    pub fn reinsert(self, priority: P) {
        heap::Vacancy {
            owner: self.entry.owner,
            value: self.entry.value.identity,
        }
        .prepare(priority)
        .store();
    }
}
