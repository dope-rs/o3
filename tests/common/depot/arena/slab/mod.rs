use std::cell::Cell;

use o3::depot::{
    self,
    arena::slab::{self, key},
};

pub mod external;

pub enum Short {}

pub enum Tag {}

#[repr(align(64))]
pub struct Aligned(pub u8);

pub enum Sent {}

pub struct Slot<'a> {
    table: &'a mut slab::Store<u64>,
    key: key::Handle,
}

unsafe impl depot::raw::Source<Sent> for Slot<'_> {
    type Key = key::Handle;

    fn key(&self) -> Self::Key {
        self.key
    }
}

impl<'a> Slot<'a> {
    /// # Safety
    /// `key` must name a live initialized slot in the retained exact owner.
    pub unsafe fn new(table: &'a mut slab::Store<u64>, key: key::Handle) -> Self {
        Self { table, key }
    }

    pub fn into_owner(self) -> &'a mut slab::Store<u64> {
        self.table
    }

    pub fn owner(&self) -> &slab::Store<u64> {
        self.table
    }
}

pub struct Family {
    pub value: Option<Box<u64>>,
    pub links: [Cell<u64>; 3],
}
