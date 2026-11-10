use o3::{depot::queue::links, mem::slot::Storage};

pub mod keyed;
pub mod slab;

#[derive(Clone, Copy)]
pub struct Model {
    pub parked: bool,
    pub key: u64,
    pub value: u64,
}

#[derive(Eq, PartialEq)]
pub struct Affine(pub [u64; 2]);

pub fn storage<T>(index: links::Index) -> Storage<T, usize> {
    Storage::new(index.get())
}
