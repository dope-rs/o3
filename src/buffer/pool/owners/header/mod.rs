use std::{cell, ops};

use crate::buffer::pool::owners::Free;

mod shape;

pub(in crate::buffer::pool) use shape::Shape;

#[derive(Clone, Copy)]
enum State {
    Owned,
    Detached,
}

#[repr(C, align(8))]
pub(in crate::buffer::pool) struct Header<K = super::Plain> {
    state: cell::Cell<State>,
    pub(super) free: Free,
    pub(super) available: cell::Cell<u32>,
    shape: super::Shape<K>,
}

impl<K> Header<K> {
    pub(super) const fn new(shape: super::Shape<K>) -> Self {
        let slots = shape.slots();
        Self {
            state: cell::Cell::new(State::Owned),
            free: Free::new(slots),
            available: cell::Cell::new(slots),
            shape,
        }
    }
}

impl<K: crate::buffer::raw::Backing> Header<K> {
    pub(in crate::buffer::pool) fn release<S>(core: super::Ptr<K, S>) {
        core.header().state.set(State::Detached);
        Self::retire(core);
    }

    pub(super) fn retire<S>(core: super::Ptr<K, S>) {
        let header = core.header();
        if header.available.get() == header.slots() && matches!(header.state.get(), State::Detached)
        {
            // SAFETY: this exact core has no root or live slot remaining; the
            // terminal ownership check immediately consumes that allocation.
            unsafe { K::release(core) };
        }
    }
}

impl<K> ops::Deref for Header<K> {
    type Target = super::Shape<K>;

    fn deref(&self) -> &Self::Target {
        &self.shape
    }
}

const _: () = {
    assert!(size_of::<Header>() == 32);
    assert!(size_of::<Header<super::Accounted>>() == 32);
    assert!(size_of::<Header<super::Resident<'static>>>() == 32);
    assert!(align_of::<Header>() == 8);
    assert!(align_of::<Header>() >= align_of::<super::Slot>());
    assert!(align_of::<Header>() >= align_of::<super::Accounts>());
    assert!(size_of::<super::Slot>().is_multiple_of(align_of::<super::Accounts>()));
    assert!(size_of::<super::Accounts>().is_multiple_of(align_of::<super::Account>()));
};
