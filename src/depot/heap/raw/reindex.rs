use std::{marker, mem};

use crate::depot::heap;

pub(in crate::depot) struct Reindex<'a, S: heap::raw::Store> {
    positions: &'a mut [S::Position],
    owner: marker::PhantomData<&'a mut S>,
}

impl<'a, S: heap::raw::Store> Reindex<'a, S> {
    pub(super) fn new(map: &'a mut [S::Position]) -> Self {
        Self {
            positions: map,
            owner: marker::PhantomData,
        }
    }
}

impl<S: heap::raw::Store> Reindex<'_, S> {
    pub(in crate::depot) unsafe fn moved(
        &mut self,
        value: &heap::Stored<S::Key, S::Priority>,
        position: &heap::raw::Position<'_, heap::Stored<S::Key, S::Priority>>,
    ) {
        use crate::depot::raw::{Address as _, Word as _};

        let index = value.identity.index();
        unsafe {
            *self.positions.get_unchecked_mut(index) = S::Position::from_offset(position.offset())
        };
    }

    pub(in crate::depot) unsafe fn removed(&mut self, value: &heap::Stored<S::Key, S::Priority>) {
        use crate::depot::raw::{Address as _, Link as _};

        let index = value.identity.index();
        unsafe { *self.positions.get_unchecked_mut(index) = S::Position::NONE };
    }
}

const _: () = {
    use heap::{hole::Step, indexed::Min};

    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<heap::raw::Hole<&'static mut Min<usize>>>() == 4 * word);
    assert!(mem::size_of::<heap::Entry<&'static mut Min<usize>>>() == 2 * word);
    assert!(mem::size_of::<Step<'static, &'static mut Min<usize>>>() == 2 * word);
    assert!(mem::size_of::<Option<Step<'static, &'static mut Min<usize>>>>() == 2 * word);
    assert!(mem::size_of::<heap::raw::Position<'static, usize>>() == 2 * word);
};
