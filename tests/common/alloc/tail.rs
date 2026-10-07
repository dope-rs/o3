use std::cell::Cell;

#[repr(align(128))]
pub struct Entry<'a> {
    pub index: usize,
    pub value: Cell<usize>,
    pub drops: Cell<Option<&'a Cell<Option<usize>>>>,
}

#[repr(align(64))]
pub struct Header<'a> {
    pub drops: &'a Cell<Option<usize>>,
    pub observed: &'a Cell<Option<Option<usize>>>,
}

impl Entry<'_> {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            value: Cell::new(0),
            drops: Cell::new(None),
        }
    }
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        if let Some(drops) = self.drops.get() {
            drops.set(drops.get().and_then(|count| count.checked_add(1)));
        }
    }
}

impl Drop for Header<'_> {
    fn drop(&mut self) {
        self.observed.set(Some(self.drops.get()));
        self.drops
            .set(self.drops.get().and_then(|count| count.checked_add(1)));
    }
}
