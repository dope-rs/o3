use std::{mem, num, ptr};

use storage::alloc;

use crate::buffer::{resident, storage, storage::regions};

#[repr(transparent)]
#[derive(Debug)]
pub(in crate::buffer) struct Heap<P: crate::buffer::raw::Policy = ()> {
    pointer: ptr::NonNull<crate::buffer::raw::Header<P>>,
}

impl<P: crate::buffer::raw::Policy> Heap<P> {
    pub(in crate::buffer) fn try_new(
        seed: P::Seed,
        capacity: num::NonZero<u32>,
    ) -> Result<Self, alloc::Error<P::Failure>> {
        use crate::buffer::raw::Token as _;

        let debit = P::try_acquire(crate::buffer::raw::Debit::new(seed, capacity))
            .map_err(alloc::Error::Policy)?;
        let capacity = debit.debit().amount();
        let layout = crate::buffer::raw::Header::<P>::layout(capacity);
        // SAFETY: this admitted header includes a nonzero byte capacity.
        let pointer = unsafe { crate::alloc::raw::try_new(layout, false) }
            .map_err(alloc::Error::Allocation)?;
        let (policy, capacity) = debit.commit();
        // SAFETY: the fresh region fits this header and the exact committed
        // capacity; initialization is infallible and precedes owner publication.
        unsafe {
            pointer.write(crate::buffer::raw::Header {
                prefix: regions::Prefix::one(capacity),
                policy,
            });
        }
        Ok(Self { pointer })
    }

    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        // SAFETY: Heap retains this exact initialized header exclusively.
        unsafe { self.pointer.as_ref() }.capacity()
    }

    pub(in crate::buffer) fn try_reserve(
        &mut self,
        target: num::NonZero<u32>,
    ) -> Result<(), alloc::Error<P::Failure>> {
        // SAFETY: Heap retains this exact initialized header exclusively.
        let header = unsafe { self.pointer.as_ref() };
        let Some(amount) = num::NonZero::new(target.get().saturating_sub(header.capacity().get()))
        else {
            return Ok(());
        };
        let debit = P::try_acquire(crate::buffer::raw::Debit::grow(&header.policy, amount))
            .map_err(alloc::Error::Policy)?;
        // SAFETY: this sole owner retains the source policy's exact admitted delta.
        self.pointer =
            unsafe { crate::buffer::raw::Growth::try_resize(self.pointer, target, debit) }
                .map_err(alloc::Error::Allocation)?;
        Ok(())
    }
}

impl<P: crate::buffer::raw::Policy> Heap<P> {
    pub(in crate::buffer) fn as_ptr(&self) -> *const mem::MaybeUninit<u8> {
        crate::buffer::raw::Header::data(self.pointer)
            .cast()
            .as_ptr()
    }

    pub(in crate::buffer) fn as_mut_ptr(&mut self) -> *mut mem::MaybeUninit<u8> {
        crate::buffer::raw::Header::data(self.pointer)
            .cast()
            .as_ptr()
    }

    pub(in crate::buffer) fn into_shared(self) -> crate::buffer::raw::Rc<P> {
        let this = mem::ManuallyDrop::new(self);
        // SAFETY: this unique initialized header transfers its initial counted edge.
        unsafe { crate::buffer::raw::Rc::from_raw(this.pointer) }
    }
}

impl<P: crate::buffer::raw::Policy> Drop for Heap<P> {
    fn drop(&mut self) {
        // SAFETY: Drop transfers this sole allocation owner once; its pointer
        // field has no remaining destruction obligation.
        unsafe { crate::buffer::raw::Header::destroy(self.pointer) };
    }
}

const _: () = assert!(size_of::<Heap<()>>() == size_of::<ptr::NonNull<()>>());
const _: () = assert!(size_of::<Option<Heap<()>>>() == size_of::<ptr::NonNull<()>>());
impl<'d> Heap<resident::Lease<'d>> {
    pub(in crate::buffer) fn try_compact(
        &mut self,
        capacity: num::NonZero<u32>,
    ) -> Result<(), crate::alloc::Exhausted> {
        let current = self.capacity();
        if capacity == current {
            return Ok(());
        }
        let old = crate::buffer::raw::Header::<resident::Lease<'d>>::layout(current);
        let new = crate::buffer::raw::Header::<resident::Lease<'d>>::layout(capacity);
        // SAFETY: Heap owns the live pointer-layout pair exclusively, and the
        // caller retains this allocation's nonzero initialized prefix.
        self.pointer = unsafe { crate::alloc::raw::resize(old, self.pointer.cast(), new)? }.cast();
        // SAFETY: the successful shrink retains this initialized header exclusively.
        let header = unsafe { self.pointer.as_mut() };
        // SAFETY: this compaction exclusively owns the header's configuration.
        unsafe { header.set_capacity(capacity) };
        resident::Domain::shrink(&mut header.policy, capacity);
        Ok(())
    }
}
