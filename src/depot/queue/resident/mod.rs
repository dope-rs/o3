//! Fixed-domain resident values scheduled through an insertion-ordered round-robin
//! lane ring.

use std::{cell, marker, mem, num};

use lanes::{Key, Lane, Lanes, Parking, Plain, Removal};
use lease::{Lease, Target};

use crate::{
    alloc,
    depot::{capacity, queue::links},
};
pub mod raw;

pub mod fifo;
pub mod handoff;
mod lanes;
mod lease;

/// A lane extent fixed by the storage constructor.
#[derive(Clone, Copy)]
pub struct Fixed<const N: usize>;

/// The immutable lane extent retained by its storage type.
pub trait Extent {
    fn count(self) -> num::NonZero<usize>;
}

impl Extent for num::NonZero<usize> {
    fn count(self) -> num::NonZero<usize> {
        self
    }
}

impl<const N: usize> Extent for Fixed<N> {
    fn count(self) -> num::NonZero<usize> {
        const { assert!(N > 0 && N <= u32::MAX as usize) };
        num::NonZero::<usize>::MIN.saturating_add(N - 1)
    }
}

/// Exact failed indexed lookup against one resident extent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds<N> {
    pub index: usize,
    pub len: N,
}

impl<N> Bounds<N> {
    pub const fn new(index: usize, len: N) -> Self {
        Self { index, len }
    }
}

/// Exact failure to build one resident lane store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Capacity(capacity::Error),
    Exhausted(alloc::Exhausted),
    Overflow(alloc::Overflow),
}

/// Exact outcome of one bounded drain transition.
#[derive(Eq, PartialEq)]
#[must_use = "a drained item remains caller-owned"]
pub enum Drain<T> {
    Done,
    Item(T),
}

/// Exact outcome of one single-item take transition.
#[derive(Eq, PartialEq)]
#[must_use = "a taken item remains caller-owned"]
pub enum Take<T> {
    Empty,
    Item(T),
}

#[derive(Default)]
#[repr(transparent)]
pub struct Draining(cell::Cell<bool>);

impl Draining {
    pub fn begin(&self) {
        self.0.set(true);
    }

    pub fn is_active(&self) -> bool {
        self.0.get()
    }

    pub fn drain<T, F>(&self, take: F) -> Drain<T>
    where
        F: FnOnce() -> Option<T>,
    {
        if !self.is_active() {
            return Drain::Done;
        }
        let Some(value) = take() else {
            self.0.set(false);
            return Drain::Done;
        };
        Drain::Item(value)
    }
}

impl From<alloc::Error> for Error {
    fn from(error: alloc::Error) -> Self {
        match error {
            alloc::Error::Exhausted => Self::Exhausted(alloc::Exhausted),
            alloc::Error::Overflow => Self::Overflow(alloc::Overflow),
        }
    }
}

#[repr(C)]
struct Capacity<T, L = num::NonZero<usize>> {
    count: capacity::Count,
    lanes: num::NonZero<u32>,
    ownership: marker::PhantomData<(raw::Node<T>, raw::Node<links::Chain>, L)>,
}

impl<T, L> Capacity<T, L> {
    fn new(capacity: usize, lanes: L) -> Result<Self, Error>
    where
        L: Extent,
    {
        let count = capacity::Count::try_from(capacity).map_err(Error::Capacity)?;
        let lanes = capacity::Count::try_nonzero(lanes.count()).map_err(Error::Capacity)?;
        alloc::raw::Bundle::try_layout::<Self>(count.get(), lanes.get() as usize)
            .map_err(Error::Overflow)?;
        Ok(Self {
            count,
            lanes,
            ownership: marker::PhantomData,
        })
    }

    const fn lanes(&self) -> num::NonZero<u32> {
        self.lanes
    }
}

// SAFETY: `new` admits both extents against this exact Bundle layout once;
// the retained count and lane count remain unchanged through allocation.
unsafe impl<T, L> alloc::raw::Shape for Capacity<T, L> {
    type Root = raw::Node<T>;
    type Tail = raw::Node<links::Chain>;
    type Extra = ();
    type Last = ();

    fn len(&self) -> usize {
        self.count.get()
    }

    fn tail_len(&self) -> usize {
        self.lanes.get() as usize
    }
}

const _: () = {
    assert!(mem::size_of::<Bounds<usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Error>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Draining>() == mem::size_of::<bool>());
    assert!(mem::size_of::<Drain<num::NonZero<usize>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Take<num::NonZero<usize>>>() == mem::size_of::<usize>());
};
