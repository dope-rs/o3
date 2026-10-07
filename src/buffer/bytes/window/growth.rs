use std::num;

use crate::buffer::bytes::window;

/// Exact window growth bounded by the window's type-level ceiling.
#[derive(Clone, Copy)]
#[repr(C)]
pub(in crate::buffer) struct Growth<const MAX: usize> {
    pub(in crate::buffer::bytes::window) capacity: window::Capacity<MAX>,
    pub(in crate::buffer::bytes::window) amount: num::NonZero<u32>,
}

impl<const MAX: usize> Growth<MAX> {
    pub(in crate::buffer::bytes::window) fn new(
        current: window::Capacity<MAX>,
        target: window::Capacity<MAX>,
    ) -> Option<Self> {
        let amount = num::NonZero::new(target.get().checked_sub(current.get())?)?;
        Some(Self {
            capacity: target,
            amount,
        })
    }

    pub(in crate::buffer) const fn capacity(self) -> window::Capacity<MAX> {
        self.capacity
    }

    pub(in crate::buffer) const fn amount(self) -> num::NonZero<u32> {
        self.amount
    }
}

const _: () = {
    assert!(size_of::<Growth<1>>() == size_of::<u64>());
    assert!(size_of::<Option<Growth<1>>>() == size_of::<Growth<1>>());
};
