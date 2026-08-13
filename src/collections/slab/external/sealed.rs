use crate::collections::{self, slab};

pub(crate) trait Backing: Sized {
    fn external(capacity: slab::Capacity) -> Result<Self, collections::AllocationError>;
}

impl<T, Tag, const MAX: u32, const PARTITIONS: usize> Backing
    for slab::Exclusive<T, Tag, MAX, true, PARTITIONS>
{
    fn external(capacity: slab::Capacity) -> Result<Self, collections::AllocationError> {
        unsafe { slab::raw::Recycling::try_with_capacity_recycling(capacity) }
    }
}

impl<T, Tag, const MAX: u32> Backing for slab::Cell<T, Tag, MAX, true> {
    fn external(capacity: slab::Capacity) -> Result<Self, collections::AllocationError> {
        unsafe { slab::raw::Recycling::try_with_capacity_recycling(capacity) }
    }
}
