use std::{mem, num};

use crate::{
    buffer,
    buffer::{
        pool,
        pool::{owners, state},
    },
};

/// Owns one live slot edge and its initialized prefix. The backing and prefix
/// cannot be replaced independently through this capability.
#[derive(Debug)]
#[repr(C)]
pub struct Lease<K: crate::buffer::raw::Backing = owners::Plain, M = pool::Owned, S = state::Uninit>
{
    pub(in crate::buffer::pool) slots: owners::Ptr<K, S>,
    pub(in crate::buffer::pool) key: owners::Key,
    pub(in crate::buffer::pool) len: u32,
    pub(in crate::buffer::pool) root: M,
}

impl<K: crate::buffer::raw::Backing, M, S> Lease<K, M, S> {
    pub const fn len(&self) -> usize {
        self.len as usize
    }
    pub fn capacity(&self) -> num::NonZero<u32> {
        self.entry().capacity()
    }

    pub fn as_slice(&self) -> &[u8] {
        buffer::raw::Prefix::as_slice(self)
    }

    pub(in crate::buffer) const fn entry(&self) -> owners::Entry<'_, K, S> {
        owners::Entry::new(self)
    }
}

impl<K: crate::buffer::raw::Backing, M, S> Drop for Lease<K, M, S> {
    fn drop(&mut self) {
        // SAFETY: dropping Span transfers its exact live slot edge once.
        unsafe { self.entry().release() };
    }
}

impl<K: crate::buffer::raw::Backing, M, S: state::State> Lease<K, M, S> {
    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }
}

impl<L, M, S: state::State> Lease<owners::Plain<L>, M, S> {
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        buffer::raw::Exclusive::as_mut_slice(self)
    }
}

impl<K: crate::buffer::raw::Backing, M, S> Lease<K, M, S> {
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// SAFETY: the lease retains this exact live slot and its initialized prefix;
// shared access cannot change the prefix, including for frozen leases.
unsafe impl<K: crate::buffer::raw::Backing, M, S> buffer::raw::Prefix for pool::Lease<K, M, S> {
    fn prefix(&self) -> (*mut mem::MaybeUninit<u8>, u32) {
        let pointer = self.entry().data().cast();
        (pointer, self.len)
    }
}

// SAFETY: sharing a plain slot consumes its writable lease. Resident slots can
// retain prefix aliases through accounts and do not carry this proof. The length
// word is inline in this lease, disjoint from its slot, and bounded by capacity.
unsafe impl<L, M, S: state::State> buffer::raw::Exclusive for pool::Lease<owners::Plain<L>, M, S> {
    fn as_mut_ptr(&mut self) -> *mut mem::MaybeUninit<u8> {
        self.entry().data().cast()
    }

    fn len_mut(&mut self) -> &mut u32 {
        &mut self.len
    }

    fn capacity(&self) -> u32 {
        self.capacity().get()
    }
}
