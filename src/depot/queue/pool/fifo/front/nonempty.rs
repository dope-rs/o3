use std::mem;

use crate::depot::{
    arena::linked,
    queue::{
        links,
        pool::fifo::{Fifo, Front},
    },
};

/// Sole ownership of a detached nonempty FIFO partition.
pub struct Nonempty<'pool, T, const TRACK: bool = true> {
    pub(super) fifo: Fifo<'pool, T, TRACK>,
}

impl<T, const TRACK: bool> Nonempty<'_, T, TRACK> {
    pub fn iter(&self) -> linked::Iter<'_, T> {
        self.fifo.iter()
    }

    pub fn pop(mut self) -> (T, Option<Self>) {
        // SAFETY: this sole owner retains the nonempty partition admitted by take or pop.
        let index = unsafe { links::Index::new_raw(self.fifo.links.head_raw().raw()) };
        let value = Front::new(&mut self.fifo, index).pop();
        let next = (!self.fifo.links.is_empty()).then_some(self);
        (value, next)
    }
}

const _: () = {
    assert!(mem::size_of::<Nonempty<'static, u64>>() == 2 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Option<Nonempty<'static, u64>>>()
            == mem::size_of::<Nonempty<'static, u64>>()
    );
};
