use std::{cell, ptr};

use crate::depot::arena::pool::{
    self, Counted,
    recycle::{self, release},
    shared,
};

// SAFETY: the policy belongs to this initialized view, and the original pool
// owner stays live until the counted edge's retained lifetime ends.
unsafe impl<
    T: crate::mem::raw::Edge,
    O: crate::depot::raw::PoolSharing<T>,
    P: crate::depot::raw::PoolPolicy<T, O>,
> crate::mem::raw::Target for Counted<T, O, P>
{
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        unsafe { Self::destroy(pointer) };
    }
}

unsafe impl<T> crate::depot::raw::PoolSharing<shared::Resident<T>> for shared::Header<T> {}

// SAFETY: the header and its initialized, disjoint slots share one stable allocation.
unsafe impl<T> crate::depot::raw::PoolOwner<T> for recycle::Header<T> {
    unsafe fn free(&self) -> &cell::Cell<*mut pool::Entry<T, Self>> {
        self.free_list()
    }
}

// SAFETY: the initialized slots and their header remain in one stable allocation.
unsafe impl<T: recycle::Recycle, C> crate::depot::raw::PoolOwner<T> for release::Header<T, C> {
    unsafe fn free(&self) -> &cell::Cell<*mut pool::Entry<T, Self>> {
        self.free_list()
    }
}

mod mutable;
mod owner;
mod policy;
mod sharing;

pub(in crate::depot) use mutable::PoolMutable;
pub(in crate::depot) use owner::PoolOwner;
pub(in crate::depot) use policy::PoolPolicy;
pub(in crate::depot) use sharing::PoolSharing;

// SAFETY: every resident slot owns exactly this reference domain while active.
unsafe impl<T> crate::mem::raw::Edge for shared::Resident<T> {
    fn count(&self) -> &crate::mem::raw::Count {
        &self.references
    }
}
