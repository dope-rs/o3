use std::cell::Cell;

use crate::marker::ThreadBound;

pub struct Scratch<T> {
    slot: Cell<Vec<T>>,
    _thread: ThreadBound,
}

impl<T> Scratch<T> {
    pub const fn new() -> Self {
        Self {
            slot: Cell::new(Vec::new()),
            _thread: ThreadBound::NEW,
        }
    }

    pub fn take(&self) -> Vec<T> {
        let mut out = self.slot.take();
        out.clear();
        out
    }

    pub fn put(&self, mut buf: Vec<T>) {
        buf.clear();
        let slot = self.slot.take();
        if buf.capacity() > slot.capacity() {
            self.slot.set(buf);
        } else {
            self.slot.set(slot);
        }
    }
}

impl<T> Default for Scratch<T> {
    fn default() -> Self {
        Self::new()
    }
}
