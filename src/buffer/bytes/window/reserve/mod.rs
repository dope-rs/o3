//! Rings that retain physical space for one fixed-size terminal write.
//! Ring allocation operations.

use std::{marker, mem, num};

pub use append::Append;
pub use error::Error;
pub use plan::Plan;
pub use prefix::Prefix;

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

/// An owned ring whose open state always retains its terminal reservation.
pub struct Ring<const N: usize> {
    buf: array::Block<mem::MaybeUninit<u8>, num::NonZero<usize>>,
    thread: marker::PhantomData<ThreadBound>,
    state: State<usize>,
    closed: bool,
}

impl<const N: usize> Ring<N> {
    pub fn try_new(plan: Plan<N>) -> Result<Self, alloc::Exhausted> {
        Ok(Self {
            buf: plan.capacity.try_allocate(())?,
            thread: marker::PhantomData,
            state: State::EMPTY,
            closed: false,
        })
    }

    pub fn capacity(&self) -> usize {
        self.buf.len() - N
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.state.len()
    }

    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        self.prefix(self.len())
    }

    fn prefix(&self, len: usize) -> (&[u8], &[u8]) {
        let (start, [front, back]) = self.state.prefix(self.buf.len(), len);
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
    fn append(&mut self, src: &[u8]) {
        use buffer::raw::SliceExt as _;

        let mut next = self.state;
        // SAFETY: Append or the retained terminal reservation admitted this
        // complete source against the unchanged nonzero allocation capacity.
        let (start, [front, back]) = unsafe { next.extend(self.buf.len(), src.len()) };
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

    fn consume(&mut self, len: usize) {
        // SAFETY: Prefix retains an admitted prefix of this exact owner.
        unsafe { self.state.consume(self.buf.len(), len) };
    }

    /// Publishes the reserved terminal bytes once, without capacity validation.
    pub fn finish(&mut self, source: &[u8; N]) {
        if !self.closed {
            self.append(source);
            self.closed = true;
        }
    }
}

const _: () = {
    assert!(size_of::<Ring<17>>() == 5 * size_of::<usize>());
    assert!(size_of::<Option<Ring<17>>>() == size_of::<Ring<17>>());
};
