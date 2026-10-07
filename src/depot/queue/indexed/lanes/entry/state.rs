use std::{mem, num::NonZero};

use crate::depot::queue::{indexed::lanes, links, tagged};

/// The initialized value or exclusive insertion proof produced by one lookup.
#[must_use]
pub enum State<O, V> {
    Occupied(O),
    Vacant(V),
}

const _: () = {
    assert!(
        mem::size_of::<
            Result<
                State<
                    &'static u64,
                    crate::depot::queue::tagged::Vacant<
                        lanes::Position<'static, u64, 1, 0, NonZero<u32>>,
                    >,
                >,
                lanes::entry::Error<NonZero<u32>>,
            >,
        >() <= mem::size_of::<
            Result<
                crate::depot::queue::tagged::Vacant<
                    lanes::Position<'static, u64, 1, 0, NonZero<u32>>,
                >,
                tagged::entry::Error<NonZero<u32>>,
            >,
        >()
    );
    assert!(
        mem::size_of::<
            Result<
                State<
                    &'static u64,
                    crate::depot::queue::tagged::Vacant<
                        crate::depot::queue::tagged::Endpoint<
                            &'static mut lanes::runtime::Fifo<u64, NonZero<u32>, NonZero<u32>>,
                            links::Index,
                            false,
                        >,
                    >,
                >,
                lanes::entry::Error<NonZero<u32>>,
            >,
        >() <= mem::size_of::<
            Result<
                crate::depot::queue::tagged::Vacant<
                    crate::depot::queue::tagged::Endpoint<
                        &'static mut lanes::runtime::Fifo<u64, NonZero<u32>, NonZero<u32>>,
                        links::Index,
                        false,
                    >,
                >,
                tagged::entry::Error<NonZero<u32>>,
            >,
        >()
    );
};
