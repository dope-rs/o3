use std::{num, ptr};

use crate::buffer::storage::regions;

/// The existing charge, its admitted delta, and their exact replacement capacity.
pub struct Growth<P> {
    source: P,
    added: P,
    capacity: num::NonZero<u32>,
}

impl<P> Growth<P> {
    pub(in crate::buffer) fn into_parts(self) -> (P, P, num::NonZero<u32>) {
        (self.source, self.added, self.capacity)
    }
}

impl<P: crate::buffer::raw::Policy> Growth<P> {
    /// # Safety
    /// The caller exclusively owns this header; the debit retains its exact
    /// policy seed and covers the target capacity's positive growth delta.
    pub(super) unsafe fn try_resize(
        pointer: ptr::NonNull<crate::buffer::raw::Header<P>>,
        capacity: num::NonZero<u32>,
        debit: P::Charge,
    ) -> Result<ptr::NonNull<crate::buffer::raw::Header<P>>, crate::alloc::Exhausted> {
        // SAFETY: the caller retains this exact initialized header exclusively.
        let old = crate::buffer::raw::Header::<P>::layout(unsafe { pointer.as_ref() }.capacity());
        let new = crate::buffer::raw::Header::<P>::layout(capacity);
        // SAFETY: both nonzero layouts preserve alignment; failure leaves the
        // original owner live and drops the uncommitted growth debit.
        let pointer = unsafe { crate::alloc::raw::resize(old, pointer.cast(), new)? }.cast();
        // SAFETY: realloc preserved the initialized header and byte prefix;
        // publication transfers the old policy and exact delta without failure.
        unsafe { Self::write(pointer, pointer, capacity, debit) };
        Ok(pointer)
    }

    /// # Safety
    /// `source` transfers its initialized policy; `pointer` fits the new header
    /// and may equal `source`. The admitted debit covers the exact capacity delta.
    pub(super) unsafe fn write(
        source: ptr::NonNull<crate::buffer::raw::Header<P>>,
        pointer: ptr::NonNull<crate::buffer::raw::Header<P>>,
        capacity: num::NonZero<u32>,
        debit: P::Charge,
    ) {
        use crate::buffer::raw::Token as _;

        let (added, _) = debit.commit();
        // SAFETY: the sole source edge transfers its policy once, including when
        // resize has moved its initialized header to the destination allocation.
        let source = unsafe { ptr::read(&raw const (*source.as_ptr()).policy) };
        let (policy, capacity) = P::grow(Self {
            source,
            added,
            capacity,
        });
        // SAFETY: publication initializes the complete admitted header once.
        unsafe {
            pointer.write(crate::buffer::raw::Header {
                prefix: regions::Prefix::one(capacity),
                policy,
            });
        }
    }
}
