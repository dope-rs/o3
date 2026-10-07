use std::cell;

use crate::depot::queue::links;
/// One independently linkable stable indexed hook.
#[repr(C)]
#[derive(Debug)]
pub struct Hook {
    pub(in crate::depot) previous: cell::Cell<u32>,
    pub(in crate::depot) next: cell::Cell<u32>,
}

impl Hook {
    pub fn new(index: links::Index) -> Self {
        Self::vacant(index.raw(), links::Index::END)
    }

    /// Constructs one detached or free-chain hook from compact raw links.
    #[doc(hidden)]
    pub const fn vacant(index: u32, next: u32) -> Self {
        Self {
            previous: cell::Cell::new(index),
            next: cell::Cell::new(next),
        }
    }

    pub fn is_linked(&self, index: usize) -> bool {
        index < links::Index::END as usize && self.previous.get() != index as u32
    }

    pub fn next_index(&self) -> Option<links::Index> {
        links::Index::decode(self.next.get())
    }

    pub fn prev_index(&self) -> Option<links::Index> {
        links::Index::decode(self.previous.get())
    }

    pub fn is_vacant(&self, index: usize) -> bool {
        index < links::Index::END as usize
            && self.previous.get() == index as u32
            && self.next.get() == links::Index::END
    }

    /// Returns the detached proof for a parked hook and clears its marker.
    pub fn try_unpark(&self, index: links::Index) -> Option<links::Detached<'_>> {
        let raw = index.raw();
        if self.previous.get() != raw || self.next.get() != raw {
            return None;
        }
        self.next.set(links::Index::END);
        Some(unsafe { links::Detached::new(self, index) })
    }
}

const _: () = {
    assert!(std::mem::size_of::<Hook>() == 2 * std::mem::size_of::<u32>());
    assert!(std::mem::align_of::<Hook>() == std::mem::align_of::<u32>());
    assert!(std::mem::size_of::<&'static Hook>() == std::mem::size_of::<usize>());
};
