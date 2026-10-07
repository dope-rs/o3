use std::{mem, num};

use crate::{buffer, buffer::bytes::window::Capacity};

pub(in crate::buffer) struct State<P: crate::buffer::raw::Policy, const MAX_CAPACITY: usize> {
    pub(in crate::buffer) buf: crate::buffer::raw::Rc<P>,
    pub(in crate::buffer::bytes::window) cap: Capacity<MAX_CAPACITY>,
    pub(in crate::buffer::bytes::window) head: u32,
}

impl<P: crate::buffer::raw::Policy, const MAX_CAPACITY: usize> State<P, MAX_CAPACITY> {
    pub(in crate::buffer) fn seed(&self) -> P::Seed {
        self.buf.seed()
    }

    pub(in crate::buffer) fn len(&self) -> u32 {
        self.head
    }

    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        self.cap.nonzero()
    }

    pub(in crate::buffer) fn as_slice(&self) -> &[u8] {
        buffer::raw::Prefix::as_slice(self)
    }
}

// SAFETY: State retains the allocation and its exact initialized prefix;
// shared snapshots cannot mutate that prefix while this state is borrowed.
unsafe impl<P: crate::buffer::raw::Policy, const MAX: usize> buffer::raw::Prefix for State<P, MAX> {
    fn prefix(&self) -> (*mut mem::MaybeUninit<u8>, u32) {
        (self.buf.data().cast_mut().cast(), self.head)
    }
}

const _: () = assert!(size_of::<State<(), 1>>() == 2 * size_of::<usize>());
