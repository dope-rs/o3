use std::num::NonZero;

use crate::{base::num::bounded, depot::capacity};

/// A capacity within the shared hash-table index domain.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Plan {
    pub(in crate::depot::map) capacity: capacity::Count,
}

impl Plan {
    pub(in crate::depot::map) const MAX: NonZero<usize> =
        NonZero::<usize>::MIN.saturating_add((1 << 30) - 1);

    pub fn new(capacity: usize) -> Result<Self, capacity::Error> {
        if capacity > Self::MAX.get() {
            return Err(capacity::Error::new(capacity));
        }
        Ok(Self {
            capacity: capacity::Count::new(capacity as u32),
        })
    }

    pub const fn from_bounded<const MIN: u32, const MAX: u32>(
        value: bounded::U32<MIN, MAX>,
    ) -> Self {
        const { assert!(MAX as usize <= Self::MAX.get()) };
        Self {
            capacity: capacity::Count::new(value.get()),
        }
    }

    #[must_use]
    pub const fn fixed<const CAPACITY: usize>() -> Self {
        const {
            assert!(
                CAPACITY <= Self::MAX.get(),
                "fixed map capacity exceeds its index domain"
            )
        }
        Self {
            capacity: capacity::Count::new(CAPACITY as u32),
        }
    }

    pub const fn capacity(self) -> capacity::Count {
        self.capacity
    }

    pub(crate) const fn buckets(self) -> usize {
        let capacity = self.capacity.get();
        if capacity == 0 {
            0
        } else {
            (capacity * 2).next_power_of_two()
        }
    }
}

const _: () = assert!(size_of::<Plan>() == size_of::<u32>());
