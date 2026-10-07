use std::mem;

use crate::{base::num::Generation, depot::map::resident};

/// Live records own their initialized value; vacant records reuse only the key word.
#[repr(C)]
pub(in crate::depot::map::resident) struct Record<T, const MAX: u32> {
    pub(in crate::depot::map::resident) id: u64,
    pub(in crate::depot::map::resident) generation: Generation<MAX>,
    pub(in crate::depot::map::resident) value: mem::MaybeUninit<T>,
}

impl<T, const MAX: u32> Record<T, MAX> {
    pub(super) fn new(id: resident::Id, value: T) -> Self {
        Self {
            id: id.get(),
            generation: Generation::INITIAL,
            value: mem::MaybeUninit::new(value),
        }
    }

    pub(in crate::depot::map::resident) const fn is_live(&self) -> bool {
        self.id & resident::raw::FREE == 0
    }

    pub(in crate::depot::map::resident) fn id(&self) -> resident::Id {
        resident::Id::mask(self.id)
    }

    pub(in crate::depot::map::resident) fn matches(&self, key: resident::Key<MAX>) -> bool {
        self.is_live() && self.generation.get() == key.generation()
    }
}
