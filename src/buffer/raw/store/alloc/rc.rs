use std::{num, ops, ptr};

use crate::{
    buffer,
    buffer::{
        resident, storage,
        storage::{alloc, backing},
    },
    mem::rc,
};

#[derive(Debug)]
#[repr(transparent)]
pub(in crate::buffer) struct Rc<P: crate::buffer::raw::Policy = ()> {
    pub(super) owner: crate::mem::raw::Owner<crate::buffer::raw::Header<P>>,
}

#[repr(transparent)]
pub(in crate::buffer) struct Unique<'a, P: crate::buffer::raw::Policy = ()>(&'a mut Rc<P>);

impl<P: crate::buffer::raw::Policy> Rc<P> {
    pub(in crate::buffer) fn header(&self) -> &crate::buffer::raw::Header<P> {
        self.owner.get()
    }

    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        self.header().capacity()
    }

    pub(in crate::buffer) fn seed(&self) -> P::Seed {
        self.header().policy.seed()
    }

    pub(in crate::buffer) fn data(&self) -> *const u8 {
        crate::buffer::raw::Header::data(self.owner.as_ptr()).as_ptr()
    }

    pub(in crate::buffer) fn is_unique(&self) -> bool {
        use crate::mem::raw::Edge as _;

        self.header().prefix.is_unique()
    }

    pub(in crate::buffer) fn into_raw(self) -> ptr::NonNull<crate::buffer::raw::Header<P>> {
        self.owner.into_raw()
    }

    /// # Safety
    /// `pointer` transfers one live shared owner edge for this exact policy.
    pub(in crate::buffer) unsafe fn from_raw(
        pointer: ptr::NonNull<crate::buffer::raw::Header<P>>,
    ) -> Self {
        Self {
            // SAFETY: the caller transfers this exact initialized counted edge.
            owner: unsafe { crate::mem::raw::Owner::from_raw(pointer) },
        }
    }
}

impl Rc<resident::Lease<'_>> {
    pub(in crate::buffer) fn try_own(self) -> Result<Rc, Self> {
        use crate::buffer::{raw::Header, resident::Lease};

        const EXTRA: u32 = {
            assert!(size_of::<Header<Lease<'_>>>() >= size_of::<Header<()>>());
            let extra = size_of::<Header<Lease<'_>>>() - size_of::<Header<()>>();
            assert!(extra <= u32::MAX as usize);
            extra as u32
        };
        let original = self.capacity();
        let Some(capacity) = original.checked_add(EXTRA) else {
            return Err(self);
        };
        if !self.is_unique() || align_of::<Header<Lease<'_>>>() != align_of::<Header<()>>() {
            return Err(self);
        }
        let pointer = self.into_raw();
        // SAFETY: the sole edge owns this policy exclusively. Refunding it ends
        // the resident-domain borrow before the same allocation becomes plain.
        unsafe { ptr::drop_in_place(&raw mut (*pointer.as_ptr()).policy) };
        let mut pointer = pointer.cast::<Header<()>>();
        // SAFETY: admission proved identical allocation size and alignment. The
        // initialized common prefix retains its unique counted edge. Former policy
        // bytes become spare capacity; the initialized byte extent stays unchanged.
        unsafe {
            pointer.as_mut().set_capacity(capacity);
            Ok(Rc::from_raw(pointer))
        }
    }
}

impl<'a, P: crate::buffer::raw::Policy> Unique<'a, P> {
    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        self.0.capacity()
    }

    pub(in crate::buffer) fn policy(&self) -> &P {
        &self.0.header().policy
    }

    pub(in crate::buffer) fn data(&self) -> *const u8 {
        self.0.data()
    }

    pub(in crate::buffer) fn slot(self) -> &'a mut Rc<P> {
        self.0
    }

    /// Resizes this unique owner using the admitted target and exact delta debit.
    pub(in crate::buffer) fn try_resize(
        &mut self,
        target: num::NonZero<u32>,
        debit: P::Charge,
    ) -> Result<(), crate::alloc::Exhausted> {
        let pointer = self.0.owner.as_ptr();
        // SAFETY: Unique retains the sole edge and the caller admitted this
        // target with its exact policy delta. Failure preserves the old owner.
        let pointer = unsafe { crate::buffer::raw::Growth::try_resize(pointer, target, debit)? };
        // SAFETY: successful resize transferred the sole edge. Replace its stale
        // representation without dropping it or accessing its former address.
        unsafe {
            ptr::write(self.0, Rc::from_raw(pointer));
        }
        Ok(())
    }

    /// Copies the admitted range and tail, then publishes their fresh allocation.
    pub(in crate::buffer) fn replace<I: crate::buffer::raw::SliceExt>(
        self,
        pointer: ptr::NonNull<crate::buffer::raw::Header<P>>,
        target: num::NonZero<u32>,
        debit: P::Charge,
        range: ops::Range<u32>,
        tail: I,
    ) {
        use crate::buffer::raw::SliceExt as _;

        let len = (range.end - range.start) as usize;
        let dst = crate::buffer::raw::Header::data(pointer);
        // SAFETY: Unique keeps the exact source alive and distinct from the
        // fresh allocation; Resize proves both copied ranges.
        unsafe {
            let source = buffer::raw::range(self.data(), range.start as usize, len);
            (source, tail).copy_to(dst.as_ptr());
        }
        let current = self.capacity();
        let slot = self.slot();
        // SAFETY: Unique transfers the sole edge out of this exclusively held
        // slot. Publication restores the slot before any subsequent access.
        let old = unsafe { ptr::read(slot) }.into_raw();
        // SAFETY: the growth debit carries the source policy's exact seed. The
        // fresh header is initialized once before the old region is freed.
        unsafe {
            crate::buffer::raw::Growth::write(old, pointer, target, debit);
            ptr::write(slot, crate::buffer::raw::Rc::from_raw(pointer));
            crate::buffer::raw::Header::free(old, current);
        }
    }
}

// SAFETY: retaining Rc preserves its allocation and the policy's owner lifetime.
unsafe impl<P: crate::buffer::raw::Policy> buffer::raw::Retain for Rc<P> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            owner: self.owner.try_clone()?,
        })
    }
}

const _: () = assert!(size_of::<Rc<()>>() == size_of::<ptr::NonNull<()>>());
const _: () = assert!(size_of::<Option<Rc<()>>>() == size_of::<ptr::NonNull<()>>());
const _: () = assert!(size_of::<Unique<'static, ()>>() == size_of::<ptr::NonNull<()>>());

impl<P: crate::buffer::raw::Policy> Rc<P> {
    /// Selects the exact access this allocation currently admits.
    pub(in crate::buffer) fn access(&mut self) -> alloc::Access<'_, P> {
        if self.is_unique() {
            alloc::Access::Unique(Unique(self))
        } else {
            alloc::Access::Shared
        }
    }
}

impl<P: buffer::raw::Policy> Rc<P> {
    /// Transfers this edge into an initialized allocation range.
    /// # Safety
    /// `start..start + len` must be initialized and within this allocation.
    pub(in crate::buffer) unsafe fn into_range(
        self,
        start: u32,
        len: u32,
    ) -> storage::Shared<Self> {
        let pointer = crate::buffer::raw::Header::data(self.owner.as_ptr());
        // SAFETY: the owned data pointer is nonnull and the offset stays inside its allocation.
        let pointer = unsafe { pointer.add(start as usize) };
        unsafe { storage::Shared::from_raw(pointer, len as usize, self) }
    }
}

impl<'d> backing::Owner<'d> for Rc<resident::Lease<'d>> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        Some(backing::Root::Resident(self.header()))
    }
}
