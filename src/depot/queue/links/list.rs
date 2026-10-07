use std::cell;

use crate::depot::queue::{indexed::keyed, links};

/// One allocation-free head-only indexed list.
#[repr(transparent)]
pub struct List(cell::Cell<u32>);

impl Default for List {
    fn default() -> Self {
        Self::new()
    }
}

impl List {
    pub const fn new() -> Self {
        Self(cell::Cell::new(links::Index::END))
    }

    pub fn front(&self) -> Option<usize> {
        self.first().map(links::Index::get)
    }

    pub fn is_empty(&self) -> bool {
        self.0.get() == links::Index::END
    }
}
impl List {
    pub(in crate::depot) fn first(&self) -> Option<links::Index> {
        links::Index::decode(self.0.get())
    }

    /// Projects the initialized endpoint of this retained nonempty list.
    pub fn front_from(source: crate::depot::raw::Proof<&Self, keyed::NonEmpty>) -> links::Index {
        // SAFETY: the consumed source proof retains this list's live non-sentinel head.
        unsafe { links::Index::new_raw(source.into_inner().0.get()) }
    }

    pub(in crate::depot) fn set(&self, index: Option<links::Index>) {
        self.0.set(links::Index::encode(index));
    }
}

const _: () = assert!(std::mem::size_of::<List>() == std::mem::size_of::<u32>());
