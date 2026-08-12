use crate::collections::queue::slot;

pub struct Front<'queue, T>(slot::Occupied<'queue, T>);

impl<'queue, T> Front<'queue, T> {
    pub(super) fn new(entry: slot::Occupied<'queue, T>) -> Self {
        Self(entry)
    }

    pub fn index(&self) -> usize {
        self.0.index()
    }

    pub fn remove(self) -> T {
        self.0.remove()
    }
}
