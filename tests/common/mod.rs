use std::{borrow::Borrow, cell::Cell};

pub mod alloc;
pub mod buffer;
pub mod depot;
pub mod mem;

pub struct Owner<'a> {
    pub drops: Counted<&'a Cell<Option<usize>>>,
    pub value: usize,
}

#[derive(Debug)]
pub struct Order<'a>(pub &'a Cell<Option<u64>>, pub u64);

impl Drop for Order<'_> {
    fn drop(&mut self) {
        self.0.set(
            self.0
                .get()
                .and_then(|order| order.checked_mul(10)?.checked_add(self.1)),
        );
    }
}

#[derive(Debug)]
pub struct Counted<C: Borrow<Cell<Option<usize>>>>(pub C);

impl<C: Borrow<Cell<Option<usize>>>> Drop for Counted<C> {
    fn drop(&mut self) {
        let count = self.0.borrow();
        count.set(count.get().and_then(|value| value.checked_add(1)));
    }
}
