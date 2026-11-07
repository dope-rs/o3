//! Compact head state over external linked slots.

use std::{cell, mem};

use crate::{
    depot,
    depot::arena::{linked, linked::slots},
};

/// Compact head for one externally stored linked list.
#[repr(transparent)]
pub struct List<Tag = ()> {
    pub(in crate::depot::arena::linked) head: slots::Index<Tag>,
}

impl<Tag> Default for List<Tag> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Tag> List<Tag> {
    pub const fn new() -> Self {
        Self {
            head: slots::Index::NONE,
        }
    }
}

const _: () = assert!(mem::size_of::<List>() == mem::size_of::<u32>());

/// Interior-mutable head of a forward chain stored in external slots.
#[repr(transparent)]
pub struct Cell<L: crate::depot::raw::Link = u32>(pub(in crate::depot) cell::Cell<L>);

/// Owner-bound proof of one present exclusive head.
#[must_use = "a present head must be consumed or left linked"]
pub struct Front<'a, L: crate::depot::raw::Link = u32> {
    owner: &'a Cell<L>,
    present: L::Item,
}

impl<L: crate::depot::raw::Link> Front<'_, L> {
    pub fn item(&self) -> L::Item {
        self.present
    }

    /// # Safety
    /// The transition owns this exact head's storage and does not reenter the head.
    pub(in crate::depot) unsafe fn pop<I, P: depot::raw::HeadPop<L, I>>(
        self,
        pop: P,
        input: I,
    ) -> P::Output {
        let (next, result) = unsafe { pop.pop(self.present, input) };
        self.owner.0.set(next);
        result
    }
}

impl<L: crate::depot::raw::Link> Cell<L> {
    pub const fn new(first: L) -> Self {
        Self(cell::Cell::new(first))
    }

    pub fn raw(&self) -> L {
        self.0.get()
    }

    pub fn front(&self) -> Option<Front<'_, L>> {
        let present = self.raw().item()?;
        Some(Front {
            owner: self,
            present,
        })
    }
}

const _: () = {
    assert!(mem::size_of::<Cell>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Cell<u16>>() == mem::size_of::<u16>());
    assert!(mem::size_of::<Front<'static>>() == 2 * mem::size_of::<usize>());
};

impl<Tag> List<Tag> {
    /// # Safety
    /// This state owns one acyclic live partition in this exact store.
    pub(in crate::depot) unsafe fn iter<'a, T>(
        &'a self,
        store: &'a slots::Store<T>,
    ) -> linked::Iter<'a, T, Tag> {
        unsafe { linked::Iter::new(store, self.head) }
    }

    /// # Safety
    /// This state owns one acyclic live partition in this exact store.
    pub(in crate::depot) unsafe fn cursor<'a, T>(
        &'a mut self,
        store: &'a slots::Store<T>,
    ) -> slots::Cursor<'a, T, Tag> {
        unsafe { slots::Cursor::new(store, self) }
    }

    /// # Safety
    /// This state owns one acyclic live partition in this exact store.
    pub(in crate::depot) unsafe fn front_entry<'a, 'b, T>(
        &'b mut self,
        store: &'a slots::Store<T>,
    ) -> Option<slots::Entry<'a, 'b, T, Tag>> {
        if self.head == slots::Index::NONE {
            return None;
        }
        Some(unsafe { slots::Entry::new(store, self) })
    }
}
