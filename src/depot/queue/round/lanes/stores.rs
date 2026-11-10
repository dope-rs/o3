use std::{cell, mem, num};

use crate::{
    alloc,
    depot::{
        capacity,
        queue::{links, resident},
        raw,
    },
};

pub(in crate::depot::queue::round) type Capacity<T> =
    resident::Capacity<T, (), cell::Cell<links::Index>>;

pub(in crate::depot::queue::round) fn try_new<T>(
    capacity: usize,
    lanes: num::NonZero<usize>,
) -> Result<Capacity<T>, alloc::Overflow> {
    const {
        assert!(mem::align_of::<raw::Node<T>>() >= mem::align_of::<raw::Node<links::Chain>>());
        assert!(
            mem::align_of::<raw::Node<links::Chain>>()
                >= mem::align_of::<cell::Cell<links::Index>>()
        );
    }
    let count = capacity::Count::try_count(capacity, alloc::Overflow)?;
    let lanes = capacity::Count::try_narrow(lanes, alloc::Overflow)?;
    Capacity::try_layout(count, lanes)
}

const _: () = {
    assert!(mem::size_of::<raw::Node<links::Chain>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<cell::Cell<links::Index>>() == mem::size_of::<u32>());
};
