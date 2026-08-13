use crate::collections::{self, slab};

/// Constructs recycling storage whose narrow physical generation is private.
pub trait Recycling<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    /// # Safety
    /// Every keyed access must first validate a wider independent identity.
    unsafe fn try_with_capacity_recycling(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError>
    where
        Self: Sized;
}
