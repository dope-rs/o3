use std::cell::Cell;

use o3::depot::{array::Index, queue::indexed::slab};

#[derive(Debug)]
pub enum Tag {}

pub const READY: Index<2> = Index::at::<0>();
pub const WAITING: Index<2> = Index::at::<1>();
pub mod layout;

pub enum Domain {}

#[derive(Debug)]
pub struct Value<'a> {
    pub id: usize,
    pub dropped: &'a Cell<Option<usize>>,
}

impl Drop for Value<'_> {
    fn drop(&mut self) {
        self.dropped.set(
            self.dropped
                .get()
                .and_then(|count| count.checked_add(self.id)),
        );
    }
}

pub struct Node {
    pub value: usize,
    pub next: Option<slab::Handle>,
}
