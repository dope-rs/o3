use std::cell;

#[repr(align(128))]
#[derive(Clone, Copy)]
pub struct Aligned(pub u8);

pub struct Owner<'a> {
    pub drops: &'a cell::Cell<Option<usize>>,
    pub value: Box<usize>,
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        self.drops.set(
            self.drops
                .get()
                .and_then(|order| order.checked_mul(10)?.checked_add(*self.value)),
        );
    }
}
