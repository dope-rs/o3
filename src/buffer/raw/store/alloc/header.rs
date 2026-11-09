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
    const VALID: () = assert!(alloc::raw::Layout::<u8>::fits_after(
        Layout::new::<Self>(),
        u32::MAX as usize
    ));

    pub(in crate::buffer) fn layout(capacity: num::NonZero<u32>) -> Layout {
        let () = Self::VALID;
        // SAFETY: VALID admits this exact header followed by any u32 byte count;
        // the byte tail starts at DATA_OFFSET and preserves the header alignment.
        unsafe {
            alloc::raw::Layout::<u8>::extend(Layout::new::<Self>(), capacity.get() as usize).0
        }
    }

    /// # Safety
    /// The fresh region fits this header and capacity. Transfer either its full
    /// debit or an existing unique owner's credit plus this exact same-policy delta.
    pub(super) unsafe fn write(
        pointer: ptr::NonNull<Self>,
        capacity: num::NonZero<u32>,
        debit: crate::buffer::raw::Debit<P>,
    ) {
        let policy = debit.commit();
        // SAFETY: the caller transfers the complete charge and fresh region;
        // initialization is infallible and precedes owner publication.
        unsafe {
            pointer.write(Self {
                prefix: regions::Prefix::one(capacity),
                policy,
            });
        }
    }

    /// # Safety
    /// Exclusively own this header; the debit retains its exact policy and
    /// covers the target capacity's positive growth delta.
    pub(super) unsafe fn try_resize(
        pointer: ptr::NonNull<Self>,
        capacity: num::NonZero<u32>,
        debit: crate::buffer::raw::Debit<P>,
    ) -> Result<ptr::NonNull<Self>, alloc::Exhausted> {
        // SAFETY: the caller retains this exact initialized header exclusively.
        let old = Self::layout(unsafe { pointer.as_ref() }.capacity());
        let new = Self::layout(capacity);
        // SAFETY: both nonzero layouts preserve alignment; failure leaves the
        // original owner live and drops only the uncommitted growth debit.
        let mut pointer = unsafe { alloc::raw::resize(old, pointer.cast(), new)? }.cast::<Self>();
        // SAFETY: realloc preserves the unique count and policy. Only capacity
        // changes, and the exact delta commits without failure before publication.
        unsafe { pointer.as_mut().set_capacity(capacity) };
        debit.commit();
        Ok(pointer)
    }

    /// # Safety
    /// Transfer the live header after `Last`, or from its sole static owner.
    pub(in crate::buffer) unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: the transferred owner keeps this initialized header live.
        let header = unsafe { pointer.as_ref() };
        let capacity = header.capacity();
        // SAFETY: terminal ownership transfers this header's complete charge once.
        unsafe { header.policy.refund(capacity) };
        // SAFETY: the charge has ended and the Copy policy has no destructor.
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
    /// Exclusively own this header with the matching allocation layout, and
    /// settle its exact policy charge for the new capacity before publication.
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
const _: () = assert!(
    size_of::<Header<crate::buffer::resident::Budget<'static>>>() == size_of::<usize>() * 2
);
