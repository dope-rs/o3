mod block_pool;
mod byte_ring;
mod bytes;
mod capacity;
mod owned;
mod pool;
mod raw;
mod ref_count;
mod rolling;
mod shared;
mod shared_pool;
mod snapshot_buf;

use std::mem::MaybeUninit;
use std::ops::Range;
use std::ptr::{self, NonNull, copy_nonoverlapping};

use crate::marker::ThreadBound;

pub use block_pool::{BlockLease, BlockPool};
pub use byte_ring::ByteRing;
pub use bytes::{Borrowed, ByteSpan, Bytes, Leased, RetainBytes, Retained};
pub use capacity::CapacityError;
pub use owned::{Block, Owned};
pub use pool::{Lease, Pool, PoolLayout, PoolLayoutError};
pub use rolling::RollingBuffer;
pub use shared::Shared;
pub use shared_pool::{Pooled, SharedLease, SharedPool};
pub use snapshot_buf::SnapshotBuf;

pub struct SpareWriter<'a> {
    ptr: NonNull<MaybeUninit<u8>>,
    capacity: usize,
    written: usize,
    target: &'a mut u32,
    _thread: ThreadBound,
}

pub enum SpareFillError<E> {
    Fill(E),
    Capacity,
}

trait RangeExt {
    fn is_within(&self, len: usize) -> bool;
}

impl RangeExt for Range<usize> {
    fn is_within(&self, len: usize) -> bool {
        self.start <= self.end && self.end <= len
    }
}

impl<'a> SpareWriter<'a> {
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.written
    }

    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    pub fn remaining(&self) -> usize {
        self.capacity - self.written
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        unsafe { self.ptr.as_ptr().add(self.written).cast() }
    }

    pub fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr.as_ptr().add(self.written), self.remaining())
        }
    }

    pub fn try_fill<E, F>(&mut self, fill: F) -> Result<(), SpareFillError<E>>
    where
        F: for<'b> FnOnce(&'b mut [MaybeUninit<u8>]) -> Result<&'b mut [u8], E>,
    {
        let expected = self.as_mut_ptr();
        let remaining = self.remaining();
        let (initialized, len) = {
            let initialized = fill(self.spare_capacity_mut()).map_err(SpareFillError::Fill)?;
            (initialized.as_ptr(), initialized.len())
        };
        if initialized != expected || len > remaining {
            return Err(SpareFillError::Capacity);
        }
        self.written += len;
        Ok(())
    }

    pub fn try_commit_initialized(&mut self, initialized: &[u8]) -> Result<(), CapacityError> {
        let attempted = self
            .written
            .checked_add(initialized.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, self.capacity))?;
        if initialized.as_ptr() != self.as_mut_ptr() || attempted > self.capacity {
            return Err(CapacityError::new(attempted, self.capacity));
        }
        self.written = attempted;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        if self.written == self.capacity {
            return Err(CapacityError::new(self.written + 1, self.capacity));
        }
        unsafe {
            self.ptr
                .as_ptr()
                .add(self.written)
                .write(MaybeUninit::new(byte))
        };
        self.written += 1;
        Ok(())
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let attempted = self
            .written
            .checked_add(src.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, self.capacity))?;
        if attempted > self.capacity {
            return Err(CapacityError::new(attempted, self.capacity));
        }
        unsafe {
            copy_nonoverlapping(
                src.as_ptr(),
                self.ptr.as_ptr().add(self.written).cast(),
                src.len(),
            )
        };
        self.written = attempted;
        Ok(())
    }

    pub fn finish(self) -> usize {
        self.written
    }

    unsafe fn new(ptr: *mut MaybeUninit<u8>, capacity: usize, target: &'a mut u32) -> Self {
        debug_assert!(capacity <= (u32::MAX - *target) as usize);
        Self {
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            capacity,
            written: 0,
            target,
            _thread: ThreadBound::NEW,
        }
    }

    fn commit(&mut self) {
        *self.target = self.target.wrapping_add(self.written as u32);
        self.written = 0;
    }
}

impl Drop for SpareWriter<'_> {
    fn drop(&mut self) {
        self.commit();
    }
}

/// # Safety
/// `buf` is valid through `*tail`, and `*head <= *tail`.
unsafe fn compact(buf: *mut MaybeUninit<u8>, head: &mut u32, tail: &mut u32) {
    if *head == 0 {
        return;
    }
    let len = (*tail - *head) as usize;
    if len != 0 {
        unsafe { ptr::copy(buf.add(*head as usize), buf, len) };
    }
    *head = 0;
    *tail = len as u32;
}

/// # Safety
/// `amount <= *tail - *head`.
unsafe fn consume(head: &mut u32, tail: &mut u32, amount: usize) {
    *head = head.wrapping_add(amount as u32);
    if *head == *tail {
        *head = 0;
        *tail = 0;
    }
}
