use std::mem;

use crate::depot::{
    capacity,
    queue::{indexed::lanes, links, tagged},
};

pub struct Position<
    'a,
    T,
    const N: usize,
    const L: usize,
    C: crate::depot::raw::Count,
    const INITIALIZED: bool = false,
> {
    pub(in crate::depot) store: &'a mut lanes::Fifo<T, N, C>,
    pub(in crate::depot) index: links::Index,
}

impl<'a, T, const N: usize, const L: usize, C: crate::depot::raw::Count>
    tagged::Vacant<Position<'a, T, N, L, C>>
{
    /// # Safety
    /// The exclusive owner proves this initialized hook is detached and in bounds.
    pub(in crate::depot) unsafe fn detached(
        store: &'a mut lanes::Fifo<T, N, C>,
        index: links::Index,
    ) -> Self {
        const { assert!(L < N) };
        unsafe { tagged::Vacant::new(Position { store, index }) }
    }
}

const _: () = {
    assert!(
        mem::size_of::<tagged::Vacant<Position<'static, (), 1, 0, capacity::Count>>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<
            tagged::Linked<Position<'static, (), 1, 0, capacity::Count, true>, lanes::Rollback>,
        >() == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<
            Option<
                tagged::Linked<Position<'static, (), 1, 0, capacity::Count, true>, lanes::Rollback>,
            >,
        >() == 2 * mem::size_of::<usize>()
    );
};

impl<T, const N: usize, const L: usize, C: crate::depot::raw::Count, const INITIALIZED: bool>
    Position<'_, T, N, L, C, INITIALIZED>
{
    pub(in crate::depot) fn parts(&self) -> (&lanes::Fifo<T, N, C>, links::Index) {
        (&*self.store, self.index)
    }
}
