use std::{cell, rc::Rc};

use o3::{
    depot::queue::{indexed::planes, links},
    mem::slot::Storage,
};

pub mod keyed;
pub mod slab;

#[derive(Clone, Copy)]
pub struct Model {
    pub parked: bool,
    pub key: u64,
    pub value: u64,
}

pub struct Ordinal;

impl planes::Seed<u64> for Ordinal {
    fn seed(&self, index: links::Index) -> u64 {
        index.get() as u64
    }
}

pub struct DropValue {
    pub drops: crate::common::Counted<Rc<cell::Cell<Option<usize>>>>,
    pub index: links::Index,
}

pub struct Drops(pub Rc<cell::Cell<Option<usize>>>);

impl planes::Seed<DropValue> for Drops {
    fn seed(&self, index: links::Index) -> DropValue {
        DropValue {
            drops: crate::common::Counted(Rc::clone(&self.0)),
            index,
        }
    }
}

#[derive(Eq, PartialEq)]
pub struct Affine(pub [u64; 2]);

impl<T> planes::Seed<Storage<T, usize>> for Ordinal {
    fn seed(&self, index: links::Index) -> Storage<T, usize> {
        Storage::new(index.get())
    }
}
