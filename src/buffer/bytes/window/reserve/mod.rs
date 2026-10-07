//! Rings that retain physical space for one fixed-size terminal write.
//! Ring allocation operations.

use std::{marker, mem, num};

pub use append::Append;
pub use error::Error;
pub use plan::Plan;
pub use prefix::Prefix;
pub use ring::Ring;

use crate::{
    alloc::{self, array},
    base::marker::ThreadBound,
    buffer,
    depot::queue::ring::State,
};

mod append;
mod error;
mod plan;
mod prefix;
mod ring;

pub(in crate::buffer::bytes::window::reserve) struct Heap {
    buf: array::Block<mem::MaybeUninit<u8>, num::NonZero<usize>>,
    thread: marker::PhantomData<ThreadBound>,
    state: State<usize>,
}

impl Heap {
    pub(in crate::buffer::bytes::window::reserve) fn try_new(
        capacity: array::Plan<u8, num::NonZero<usize>>,
    ) -> Result<Self, alloc::Exhausted> {
        Ok(Self {
            buf: capacity.try_allocate(())?,
            thread: marker::PhantomData,
            state: State::EMPTY,
        })
    }

    pub(in crate::buffer::bytes::window::reserve) fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub(in crate::buffer::bytes::window::reserve) fn len(&self) -> usize {
        self.state.len()
    }

    pub(in crate::buffer::bytes::window::reserve) fn as_slices(
        &self,
        len: usize,
    ) -> (&[u8], &[u8]) {
        let (start, [front, back]) = self.state.prefix(self.capacity(), len);
        let pointer = self.buf.as_ptr().cast::<u8>();
        // SAFETY: the ring exposes exactly its initialized front and back ranges
        // of this unchanged block allocation.
        unsafe {
            (
                buffer::raw::range(pointer, start, front),
                buffer::raw::range(pointer, 0, back),
            )
        }
    }

    /// Appends one admitted source at the logical tail.
    ///
    /// Admission proves that the source fits; this owner updates its tail and length.
    pub(in crate::buffer::bytes::window::reserve) fn append(&mut self, src: &[u8]) {
        use buffer::raw::SliceExt as _;

        let mut next = self.state;
        // SAFETY: Append or the retained terminal reservation admitted this
        // complete source against the unchanged nonzero allocation capacity.
        let (start, [front, back]) = unsafe { next.extend(self.capacity(), src.len()) };
        let (head, tail) = src.split_at(front);
        let pointer = self.buf.as_mut_ptr().cast::<u8>();
        // SAFETY: admission retains disjoint sources and enough vacant storage
        // for both slices in this unchanged block.
        unsafe { head.copy_to(pointer.add(start)) };
        if back != 0 {
            // SAFETY: the same admission leaves this disjoint back range vacant.
            unsafe { tail.copy_to(pointer) };
        }
        self.state = next;
    }

    pub(in crate::buffer::bytes::window::reserve) fn consume(&mut self, len: usize) {
        // SAFETY: Prefix retains an admitted prefix of this exact owner.
        unsafe { self.state.consume(self.capacity(), len) };
    }
}

const _: () = {
    assert!(size_of::<Heap>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<Heap>>() == size_of::<Heap>());
};
