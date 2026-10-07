use std::mem;

use nonempty::Nonempty;

use crate::depot::{
    arena::linked::slots,
    queue::{links, pool::fifo::Fifo},
};

pub(super) mod nonempty;

/// A nonempty front retaining the exact exclusive FIFO borrow.
pub struct Front<'fifo, 'pool, T, const TRACK: bool = true> {
    fifo: &'fifo mut Fifo<'pool, T, TRACK>,
    index: links::Index,
}

impl<'fifo, 'pool, T, const TRACK: bool> Front<'fifo, 'pool, T, TRACK> {
    pub(in crate::depot::queue::pool::fifo) fn new(
        fifo: &'fifo mut Fifo<'pool, T, TRACK>,
        index: links::Index,
    ) -> Self {
        Self { fifo, index }
    }

    pub fn get(&self) -> &T {
        // SAFETY: the unchanged exclusive FIFO borrow owns this initialized front.
        unsafe {
            self.fifo
                .arena
                .inner
                .value(slots::Index::<()>::new(self.index.raw()))
        }
    }

    pub fn pop(self) -> T {
        let store = &self.fifo.arena.inner;
        // SAFETY: this capability retains the exact unchanged live FIFO front.
        let index = unsafe {
            self.fifo
                .links
                .detach_store(store, slots::Index::new(self.index.raw()))
        };
        unsafe { store.take::<_, TRACK>(index) }
    }

    pub fn take(self) -> Nonempty<'pool, T, TRACK> {
        let empty = Fifo::new(self.fifo.arena);
        Nonempty {
            fifo: mem::replace(self.fifo, empty),
        }
    }
}

const _: () =
    assert!(mem::size_of::<Front<'static, 'static, u64>>() == 2 * mem::size_of::<usize>());
