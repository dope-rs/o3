use std::{alloc::Layout, num, ptr};

use crate::{alloc, buffer::storage::regions};

#[repr(C)]
#[derive(Debug)]
pub(in crate::buffer) struct Header<P: crate::buffer::raw::Policy> {
    pub(super) prefix: regions::Prefix<num::NonZero<u32>>,
    pub(super) policy: P,
}

// SAFETY: this initialized header retains its exact embedded reference domain.
unsafe impl<P: crate::buffer::raw::Policy> crate::mem::raw::Edge for Header<P> {
    fn count(&self) -> &crate::mem::raw::Count {
        crate::mem::raw::Edge::count(&self.prefix)
    }
}

// SAFETY: every counted edge retains this allocation and its initialized policy.
unsafe impl<P: crate::buffer::raw::Policy> crate::mem::raw::Target for Header<P> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: the last counted edge transfers this exact initialized allocation.
        unsafe { Self::destroy(pointer) };
    }
}

impl<P: crate::buffer::raw::Policy> Header<P> {
    pub(in crate::buffer) const DATA_OFFSET: usize = size_of::<Self>();
    const ALIGN: usize = align_of::<Self>();
    const VALID: () = assert!(alloc::raw::Layout::<u8>::fits_after(
        Layout::new::<Self>(),
        u32::MAX as usize
    ));

    pub(in crate::buffer) fn layout(capacity: num::NonZero<u32>) -> Layout {
        let () = Self::VALID;
        // SAFETY: VALID admits this exact header followed by any u32 byte count;
        // the byte tail starts at DATA_OFFSET and preserves the header alignment.
        unsafe {
            alloc::raw::Layout::<u8>::at(
                Self::DATA_OFFSET,
                capacity.get() as usize,
                num::NonZero::<usize>::MIN.saturating_add(Self::ALIGN - 1),
            )
        }
    }

    /// # Safety
    /// Transfer the live header after `Last`, or from its sole static owner.
    pub(in crate::buffer) unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: the transferred owner keeps this initialized header live.
        let header = unsafe { pointer.as_ref() };
        let capacity = header.capacity();
        // SAFETY: terminal ownership drops the complete charged policy once.
        unsafe { ptr::drop_in_place(pointer.as_ptr()) };
        // SAFETY: all initialized fields have been destroyed before disposal.
        unsafe { Self::free(pointer, capacity) };
    }

    pub(super) unsafe fn free(pointer: ptr::NonNull<Self>, capacity: num::NonZero<u32>) {
        // SAFETY: the caller transfers this exact region and has released or
        // transferred any initialized policy's charge; no destructors remain.
        unsafe { crate::alloc::raw::dealloc(Self::layout(capacity), pointer) };
    }

    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        self.prefix.word()
    }

    /// # Safety
    /// The caller exclusively owns this header's configuration state.
    pub(in crate::buffer) unsafe fn set_capacity(&mut self, capacity: num::NonZero<u32>) {
        // SAFETY: the caller retains this header exclusively.
        unsafe { self.prefix.write(capacity) };
    }

    pub(super) fn data(pointer: ptr::NonNull<Self>) -> ptr::NonNull<u8> {
        // SAFETY: every live header allocation has its byte region directly
        // after the fixed header offset.
        unsafe { pointer.cast::<u8>().add(Self::DATA_OFFSET) }
    }

    /// # Safety
    /// `pointer` names a live allocation whose complete byte region is
    /// initialized for the duration of the returned raw slice use.
    pub(in crate::buffer) unsafe fn slice(pointer: ptr::NonNull<Self>) -> ptr::NonNull<[u8]> {
        // SAFETY: the caller proves the initialized allocation header is live.
        let capacity = unsafe { pointer.as_ref() }.capacity().get() as usize;
        // SAFETY: data begins a live initialized capacity-byte allocation region.
        ptr::NonNull::slice_from_raw_parts(Self::data(pointer), capacity)
    }
}

const _: () = {
    assert!(size_of::<Header<()>>() == size_of::<regions::Prefix>());
    assert!(size_of::<regions::Prefix<num::NonZero<u32>>>() == size_of::<regions::Prefix>());
    assert!(align_of::<regions::Prefix<num::NonZero<u32>>>() == align_of::<regions::Prefix>());
};
const _: () = assert!(align_of::<Header<()>>() >= 8);
