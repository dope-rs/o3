pub(in crate::buffer) use input::Measured;

use crate::buffer::{pool::owners, storage::regions};

mod input;

use std::{marker, mem, ptr};

use crate::{buffer, buffer::pool, mem::rc};

/// One exact live pool-slot edge with its backing and root mode preserved.
pub type Edge<K = owners::Plain, M = pool::Owned> = Option<Owner<K, M>>;

#[derive(Debug)]
#[repr(transparent)]
pub struct Owner<K: crate::buffer::raw::Backing, M> {
    ptr: ptr::NonNull<regions::Prefix>,
    mode: marker::PhantomData<(K, M)>,
}

impl<K: crate::buffer::raw::Backing, M> Owner<K, M> {
    pub(in crate::buffer::pool) const fn new(ptr: ptr::NonNull<regions::Prefix>) -> Self {
        Self {
            ptr,
            mode: marker::PhantomData,
        }
    }

    pub(in crate::buffer) fn into_raw(self) -> ptr::NonNull<regions::Prefix> {
        let this = mem::ManuallyDrop::new(self);
        this.ptr
    }
}

impl<K: crate::buffer::raw::Backing, M> Drop for Owner<K, M> {
    fn drop(&mut self) {
        // SAFETY: Owner owns exactly one live pool-slot prefix edge.
        drop(unsafe { crate::mem::raw::Owner::<owners::Slot<K>>::from_raw(self.ptr.cast()) });
    }
}

// SAFETY: retaining Owner creates one edge for the same exact pool slot and
// preserves both its backing kind and root-mode lifetime.
unsafe impl<K: crate::buffer::raw::Backing, M> buffer::raw::Retain for Owner<K, M> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        use crate::mem::raw::Edge;

        // SAFETY: the borrowed Owner proves this slot prefix is active.
        unsafe { self.ptr.as_ref().try_retain() }?;
        Ok(Self {
            ptr: self.ptr,
            mode: marker::PhantomData,
        })
    }
}

const _: () = {
    assert!(size_of::<Edge>() == size_of::<usize>());
    assert!(size_of::<Edge<owners::Plain, pool::Borrowed<'static>>>() == size_of::<usize>());
    assert!(size_of::<Edge<owners::Resident<'static>>>() == size_of::<usize>());
};

impl<K: crate::buffer::raw::Backing, M> regions::End for Edge<K, M> {
    const EMPTY: Option<Self> = Some(None);
}

impl<K: crate::buffer::raw::Backing, M> buffer::Residency for owners::Edge<K, M> {
    fn resident(&self) -> usize {
        self.as_ref().map_or(0, |owner| {
            // SAFETY: Edge retains this exact active pool-slot prefix.
            unsafe { owners::capacity_of::<K>(owner.ptr.cast()) }.get() as usize
        })
    }
}

impl<'d> Owner<owners::Charged<'d>, pool::Owned> {
    pub(in crate::buffer) fn root(&self) -> buffer::storage::backing::Root<'_, 'd> {
        // SAFETY: this owner retains the exact initialized charged slot.
        let slot = unsafe {
            self.ptr
                .cast::<owners::Slot<owners::Charged<'d>>>()
                .as_ref()
        };
        buffer::storage::backing::Root::Charged(slot)
    }
}
