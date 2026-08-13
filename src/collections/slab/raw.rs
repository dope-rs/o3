use crate::collections::{
    self,
    slab::{self, key},
};

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

#[doc(hidden)]
pub trait ExternalAccess<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    fn entry_at(&self, index: u32) -> Option<(&T, key::Handle<Tag, MAX>)>;

    fn entry_at_mut(&mut self, index: u32) -> Option<(&mut T, key::Handle<Tag, MAX>)>;
}
