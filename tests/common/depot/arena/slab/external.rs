use std::{cell::Cell, marker, ptr};

use o3::depot::arena::slab::key;

pub type Generation = key::Generation<3>;
pub type Wide = key::Generation;
pub type Epoch = key::Generation<{ u8::MAX as u32 }>;

pub struct Pinned<'a> {
    pub address: Cell<*const Self>,
    drops: &'a Cell<u8>,
    _pin: marker::PhantomPinned,
}

impl<'a> Pinned<'a> {
    pub fn new(drops: &'a Cell<u8>) -> Self {
        Self {
            address: Cell::new(ptr::null()),
            drops,
            _pin: marker::PhantomPinned,
        }
    }
}

impl Drop for Pinned<'_> {
    fn drop(&mut self) {
        assert!(ptr::eq(self, self.address.get()));
        self.drops.set(self.drops.get() + 1);
    }
}
