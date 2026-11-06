use std::{cell, marker, mem, num};

use crate::{
    alloc,
    depot::{capacity, queue::links, raw},
};

#[derive(Debug)]
#[repr(C)]
pub(in crate::depot::queue::round) struct Capacity<T> {
    count: capacity::Count,
    lanes: num::NonZero<u32>,
    ownership: marker::PhantomData<(
        raw::Node<T>,
        raw::Node<links::Chain>,
        cell::Cell<links::Index>,
    )>,
}

impl<T> Capacity<T> {
    pub(in crate::depot::queue::round) fn new(
        capacity: usize,
        lanes: num::NonZero<usize>,
    ) -> Result<Self, alloc::Overflow> {
        const {
            assert!(mem::align_of::<raw::Node<T>>() >= mem::align_of::<raw::Node<links::Chain>>());
            assert!(
                mem::align_of::<raw::Node<links::Chain>>()
                    >= mem::align_of::<cell::Cell<links::Index>>()
            );
        }
        let count = capacity::Count::try_count(capacity, alloc::Overflow)?;
        let lanes = capacity::Count::try_narrow(lanes, alloc::Overflow)?;
        alloc::raw::Bundle::try_layout::<Self>(count.get(), lanes.get() as usize)?;
        Ok(Self {
            count,
            lanes,
            ownership: marker::PhantomData,
        })
    }
}

unsafe impl<T> alloc::raw::Shape for Capacity<T> {
    type Root = raw::Node<T>;
    type Tail = raw::Node<links::Chain>;
    type Extra = cell::Cell<links::Index>;
    type Last = ();

    fn len(&self) -> usize {
        self.count.get()
    }

    fn tail_len(&self) -> usize {
        self.lanes.get() as usize
    }
}

const _: () = {
    assert!(mem::size_of::<raw::Node<links::Chain>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<cell::Cell<links::Index>>() == mem::size_of::<u32>());
};
