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

/// Exclusive head of a forward chain stored in external slots.
#[repr(transparent)]
pub struct Exclusive<L: crate::depot::raw::Link = u32>(L);

/// Interior-mutable head of a forward chain stored in external slots.
#[repr(transparent)]
pub struct Cell<L: crate::depot::raw::Link = u32>(cell::Cell<L>);

/// Owner-bound proof of one present exclusive head.
#[must_use = "a present head must be consumed or left linked"]
pub struct Front<O, L: crate::depot::raw::Link = u32> {
    owner: O,
    present: crate::depot::raw::Present<L>,
}

impl<O, L: crate::depot::raw::Link> Front<O, L> {
    pub fn item(&self) -> L::Item {
        self.present.item()
    }
}

impl<L: depot::raw::Link, O: Owner<L>> Front<O, L> {
    /// # Safety
    /// The transition owns this exact head's storage and does not reenter the head.
    pub(in crate::depot) unsafe fn pop<I, P: depot::raw::HeadPop<L, I>>(
        self,
        pop: P,
        input: I,
    ) -> P::Output {
        let (next, result) = unsafe { pop.pop(self.present.item(), input) };
        self.owner.store(next);
        result
    }
}

impl<L: crate::depot::raw::Link> Exclusive<L> {
    pub const fn new(first: L) -> Self {
        Self(first)
    }

    pub const fn raw(&self) -> L {
        self.0
    }

    pub fn front(&mut self) -> Option<Front<&mut Self, L>> {
        let present = crate::depot::raw::Present::new(self.raw())?;
        Some(Front {
            owner: self,
            present,
        })
    }
}

impl<L: crate::depot::raw::Link> Cell<L> {
    pub const fn new(first: L) -> Self {
        Self(cell::Cell::new(first))
    }

    pub fn raw(&self) -> L {
        self.0.get()
    }

    pub fn front(&self) -> Option<Front<&Self, L>> {
        let present = crate::depot::raw::Present::new(self.raw())?;
        Some(Front {
            owner: self,
            present,
        })
    }
}

const _: () = {
    assert!(mem::size_of::<Exclusive>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Cell>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Exclusive<u16>>() == mem::size_of::<u16>());
    assert!(mem::size_of::<Cell<u16>>() == mem::size_of::<u16>());
    assert!(mem::align_of::<Exclusive>() == mem::align_of::<u32>());
    assert!(mem::size_of::<Option<Exclusive>>() == mem::size_of::<Option<u32>>());
    assert!(mem::size_of::<Option<Exclusive<u16>>>() == mem::size_of::<Option<u16>>());
    assert!(mem::size_of::<Front<&'static mut Exclusive>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Front<&'static Cell>>() == 2 * mem::size_of::<usize>());
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

/// A closed native head loan whose writes retain its original storage identity.
pub trait Owner<L: depot::raw::Link>: Sealed<L> {
    fn raw(&self) -> L;
    fn store(self, next: L);
}

impl<L: depot::raw::Link> Sealed<L> for &mut Exclusive<L> {}
impl<L: depot::raw::Link> Sealed<L> for &Cell<L> {}

impl<L: depot::raw::Link> Owner<L> for &mut Exclusive<L> {
    fn raw(&self) -> L {
        self.0
    }

    fn store(self, next: L) {
        self.0 = next;
    }
}

impl<L: depot::raw::Link> Owner<L> for &Cell<L> {
    fn raw(&self) -> L {
        self.0.get()
    }

    fn store(self, next: L) {
        self.0.set(next);
    }
}

mod sealed;
pub(in crate::depot) use sealed::Sealed;
