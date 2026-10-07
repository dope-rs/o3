use std::num;

use crate::{alloc::array, buffer::bytes::window::reserve};

/// A representable allocation with room for its fixed terminal write.
#[derive(Clone, Copy)]
pub struct Plan<const N: usize> {
    pub(super) capacity: array::Plan<u8, num::NonZero<usize>>,
}

impl<const N: usize> Plan<N> {
    pub fn new(capacity: num::NonZero<usize>) -> Result<Self, reserve::Error> {
        let error = reserve::Error {
            capacity,
            reserved: N,
        };
        let total = capacity.checked_add(N).ok_or(error)?;
        Ok(Self {
            capacity: array::Plan::try_new(total).map_err(|_| error)?,
        })
    }

    pub const fn capacity(self) -> num::NonZero<usize> {
        *self.capacity.extent()
    }
}

const _: () = {
    assert!(size_of::<Plan<17>>() == size_of::<usize>());
    assert!(size_of::<Option<Plan<17>>>() == size_of::<Plan<17>>());
};
