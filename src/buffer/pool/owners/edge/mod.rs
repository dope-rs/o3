pub(in crate::buffer) use input::Measured;

use crate::buffer::{pool::owners, storage::regions};

mod input;

use std::{marker, mem, ptr};

use crate::{buffer, buffer::pool, mem::rc};

/// One exact live pool-slot edge with its backing and root mode preserved.
#[derive(Debug)]
#[repr(transparent)]
pub struct Edge<K: crate::buffer::raw::Backing = owners::Plain, M = pool::Owned> {
    ptr: Option<ptr::NonNull<regions::Prefix>>,
    mode: marker::PhantomData<(K, M)>,
}

impl<K: crate::buffer::raw::Backing, M> Edge<K, M> {
    pub(in crate::buffer) const NONE: Self = Self {
        ptr: None,
        mode: marker::PhantomData,
    };

    pub(in crate::buffer::pool) const fn new(ptr: ptr::NonNull<regions::Prefix>) -> Self {
        Self {
            ptr: Some(ptr),
            mode: marker::PhantomData,
        }
    }

    pub(in crate::buffer) fn into_raw(self) -> Option<ptr::NonNull<regions::Prefix>> {
        let this = mem::ManuallyDrop::new(self);
        this.ptr
    }
}

impl<K: crate::buffer::raw::Backing, M> Drop for Edge<K, M> {
    fn drop(&mut self) {
        if let Some(ptr) = self.ptr {
            // SAFETY: Edge owns exactly one live pool-slot prefix edge.
            drop(unsafe { crate::mem::raw::Owner::<owners::Slot<K>>::from_raw(ptr.cast()) });
        }
    }
}

// SAFETY: retaining Edge creates one edge for the same exact pool slot and
// preserves both its backing kind and root-mode lifetime.
unsafe impl<K: crate::buffer::raw::Backing, M> buffer::raw::Retain for Edge<K, M> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        use crate::mem::raw::Edge;

        let pointer = self.ptr;
        if let Some(ptr) = pointer {
            // SAFETY: the borrowed Edge proves this slot prefix is active.
            unsafe { ptr.as_ref().try_retain() }?;
        }
        Ok(Self {
            ptr: pointer,
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
    const EMPTY: Option<Self> = Some(Self::NONE);
}

impl<K: crate::buffer::raw::Backing, M> buffer::Residency for owners::Edge<K, M> {
    fn resident(&self) -> usize {
        self.ptr.map_or(0, |ptr| {
            // SAFETY: Edge retains this exact active pool-slot prefix.
            unsafe { owners::capacity_of::<K>(ptr.cast()) }.get() as usize
        })
    }
}
