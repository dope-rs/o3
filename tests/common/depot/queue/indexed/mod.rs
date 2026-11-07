use std::{cell, rc::Rc};

use o3::{depot::queue::links, mem::slot::Storage};

pub mod keyed;
pub mod slab;

#[derive(Clone, Copy)]
pub struct Model {
    pub parked: bool,
    pub key: u64,
    pub value: u64,
}

pub struct DropValue {
    pub drops: crate::common::Counted<Rc<cell::Cell<Option<usize>>>>,
    pub index: links::Index,
}

pub fn drop_values(drops: Rc<cell::Cell<Option<usize>>>) -> impl Fn(links::Index) -> DropValue {
    move |index| DropValue {
        drops: crate::common::Counted(Rc::clone(&drops)),
        index,
    }
}

#[derive(Eq, PartialEq)]
pub struct Affine(pub [u64; 2]);

pub fn storage<T>(index: links::Index) -> Storage<T, usize> {
    Storage::new(index.get())
}
