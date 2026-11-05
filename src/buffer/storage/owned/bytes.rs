use std::{mem, ops, ptr};

use crate::buffer;

/// A uniquely owned, non-growing byte allocation. `CAP == 0` selects a
/// runtime capacity; a nonzero `CAP` fixes it in the type without added state.
#[derive(Debug)]
#[repr(C)]
pub struct Bytes<P: crate::buffer::raw::Policy = (), const CAP: u32 = 0> {
    pub(in crate::buffer::storage) allocation: Option<crate::buffer::raw::Heap<P>>,
    pub(in crate::buffer::storage) len: u32,
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> ops::Deref for Bytes<P, CAP> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> ops::DerefMut for Bytes<P, CAP> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> Bytes<P, CAP> {
    pub fn capacity(&self) -> usize {
        if CAP == 0 {
            match &self.allocation {
                None => 0,
                Some(heap) => heap.capacity().get() as usize,
            }
        } else {
            CAP as usize
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        buffer::raw::Prefix::as_slice(self)
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        buffer::raw::Exclusive::as_mut_slice(self)
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> buffer::raw::Space for Bytes<P, CAP> {
    fn remaining(&self) -> usize {
        self.capacity() - self.len()
    }
}

impl<const N: usize> buffer::raw::Slices<'_, N> {
    /// Replaces the suffix after `prefix` and publishes the complete initialized length.
    ///
    /// # Safety
    /// `prefix` must not exceed `target.len()`. The mathematical sum of `prefix`
    /// and this source's exact length must not exceed `target.capacity()`.
    /// Every source must remain valid and disjoint from the entire written range.
    pub unsafe fn replace(self, target: &mut Bytes, prefix: u32) {
        use buffer::raw::SliceExt as _;

        // SAFETY: this positive source fits target's runtime capacity, which is
        // zero without an allocation. The exclusive target therefore retains a heap.
        let heap = unsafe { crate::mem::raw::initialized(&target.allocation) };
        // SAFETY: the caller retains this exact disjoint suffix in that heap.
        unsafe {
            self.slices()
                .copy_to(heap.as_ptr().cast_mut().add(prefix as usize).cast())
        };
        target.len = prefix + self.len().get();
    }
}

// SAFETY: Bytes owns its heap and tracks only its initialized prefix; an
// absent heap has length zero. Shared access cannot mutate either field.
unsafe impl<P: crate::buffer::raw::Policy, const CAP: u32> buffer::raw::Prefix for Bytes<P, CAP> {
    fn prefix(&self) -> (*mut mem::MaybeUninit<u8>, u32) {
        let pointer = self
            .allocation
            .as_ref()
            .map_or(ptr::NonNull::dangling().as_ptr(), |heap| {
                heap.as_ptr().cast_mut()
            });
        (pointer, self.len)
    }
}

// SAFETY: the heap is uniquely owned, disjoint from its length word, and
// every construction and write keeps that word within its u32 capacity.
unsafe impl<P: crate::buffer::raw::Policy, const CAP: u32> buffer::raw::Exclusive
    for Bytes<P, CAP>
{
    fn as_mut_ptr(&mut self) -> *mut mem::MaybeUninit<u8> {
        self.allocation
            .as_mut()
            .map_or(ptr::NonNull::dangling().as_ptr(), |heap| heap.as_mut_ptr())
    }

    fn len_mut(&mut self) -> &mut u32 {
        &mut self.len
    }

    fn capacity(&self) -> u32 {
        Bytes::capacity(self) as u32
    }
}
