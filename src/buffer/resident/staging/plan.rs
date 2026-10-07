use std::num;

use crate::buffer::resident::staging::Error;

/// Warm working set and hard contiguous ceiling for one staging buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    warm: num::NonZero<u32>,
    limit: num::NonZero<u32>,
}

impl Plan {
    pub fn new(warm: usize, limit: usize) -> Result<Self, Error> {
        let Some(warm) = num::NonZero::<usize>::new(warm) else {
            return Err(Error::ZeroWarm);
        };
        let Some(limit) = num::NonZero::<usize>::new(limit) else {
            return Err(Error::WarmExceedsLimit { warm, limit: 0 });
        };
        if warm > limit {
            return Err(Error::WarmExceedsLimit {
                warm,
                limit: limit.get(),
            });
        }
        if limit.get() > u32::MAX as usize {
            return Err(Error::LimitTooLarge { limit });
        }
        let warm = num::NonZero::<u32>::MIN.saturating_add(warm.get() as u32 - 1);
        let limit = num::NonZero::<u32>::MIN.saturating_add(limit.get() as u32 - 1);
        Ok(Self { warm, limit })
    }

    pub const fn warm(self) -> num::NonZero<u32> {
        self.warm
    }

    pub const fn ceiling(self) -> num::NonZero<u32> {
        self.limit
    }

    pub(super) const fn bounds(self) -> (num::NonZero<u32>, num::NonZero<u32>) {
        (self.warm, self.limit)
    }
}

const _: () = {
    assert!(usize::BITS >= u32::BITS);
    assert!(size_of::<Plan>() == size_of::<u64>());
    assert!(size_of::<Option<Plan>>() == size_of::<Plan>());
};
