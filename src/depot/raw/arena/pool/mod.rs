use std::cell;

use crate::depot::arena::pool::{
    self,
    recycle::{self, release},
    shared,
};

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

pub(in crate::depot) use mutable::PoolMutable;
pub(in crate::depot) use owner::PoolOwner;
pub(in crate::depot) use policy::PoolPolicy;

// SAFETY: every resident slot owns exactly this reference domain while active.
unsafe impl<T> crate::mem::raw::Edge for shared::Resident<T> {
    fn count(&self) -> &crate::mem::raw::Count {
        &self.references
    }
}
