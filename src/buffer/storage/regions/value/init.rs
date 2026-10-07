use std::{marker, mem, num, ptr, slice};

use regions::Initialize as _;

use crate::{
    buffer,
    buffer::{resident, storage, storage::regions},
    mem::rc,
};

/// One allocation whose complete data region is initialized.
#[derive(Debug)]
#[repr(transparent)]
pub(in crate::buffer) struct Init<P: crate::buffer::raw::Policy = ()> {
    allocation: crate::buffer::raw::Rc<P>,
}

/// One complete allocation, with its plain or resident policy encoded in bit zero.
#[derive(Debug)]
#[repr(transparent)]
pub(in crate::buffer) struct Complete<'d> {
    pointer: ptr::NonNull<regions::Prefix<num::NonZero<u32>>>,
    ownership: marker::PhantomData<Init<resident::Lease<'d>>>,
}

impl<P: crate::buffer::raw::Policy> Init<P> {
    /// # Safety
    /// Every byte through `allocation.capacity()` must be initialized.
    pub(in crate::buffer) unsafe fn new(rc: crate::buffer::raw::Rc<P>) -> Self {
        Self { allocation: rc }
    }

    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        self.allocation.capacity()
    }

    pub(in crate::buffer) fn as_slice(&self) -> &[u8] {
        // SAFETY: construction proves the complete allocation is initialized,
        // and this owner keeps the header and data live for the borrow.
        unsafe { slice::from_raw_parts(self.allocation.data(), self.capacity().get() as usize) }
    }

    pub(in crate::buffer) fn into_buf(self) -> storage::Shared<crate::buffer::raw::Rc<P>> {
        let len = self.capacity();
        // SAFETY: Init transfers its fully initialized extent with its original owner.
        unsafe { self.allocation.into_range(0, len.get()) }
    }
}

impl Init<()> {
    pub(in crate::buffer) fn bind<'d>(self) -> regions::Complete<'d> {
        Complete {
            pointer: self.allocation.into_raw().cast(),
            ownership: marker::PhantomData,
        }
    }
}

impl<'d> Init<resident::Lease<'d>> {
    pub(in crate::buffer) fn bind(self) -> regions::Complete<'d> {
        Complete {
            pointer: self
                .allocation
                .into_raw()
                .cast()
                .map_addr(|address| address | 1),
            ownership: marker::PhantomData,
        }
    }

    pub(in crate::buffer) fn try_copy<const N: usize>(
        budget: &resident::Budget<'d>,
        slices: [&[u8]; N],
    ) -> Result<Option<Self>, resident::AllocError> {
        use resident::AllocError;
        let Some(slices) = buffer::raw::Slices::try_new(slices).map_err(AllocError::Capacity)?
        else {
            return Ok(None);
        };
        let init = slices.try_init(*budget)?;
        Ok(Some(init))
    }
}

const _: () = assert!(size_of::<Init>() == size_of::<usize>());

impl<'d> Complete<'d> {
    pub(in crate::buffer) fn as_slice<'a>(&'a self) -> &'a [u8] {
        let pointer = self.pointer();
        let bytes = if self.pointer.addr().get() & 1 == 0 {
            // SAFETY: the plain Init constructor transfers its entire initialized allocation.
            unsafe { crate::buffer::raw::Header::<()>::slice(pointer.cast()) }
        } else {
            // SAFETY: bit zero encodes the transferred resident Init and its exact lifetime.
            unsafe { crate::buffer::raw::Header::<resident::Lease<'d>>::slice(pointer.cast()) }
        };
        // SAFETY: this owner borrow keeps the complete initialized region live.
        unsafe { bytes.as_ref() }
    }

    pub(in crate::buffer) fn into_buf(self) -> resident::Shared<'d> {
        let this = mem::ManuallyDrop::new(self);
        let pointer = this.pointer();
        if this.pointer.addr().get() & 1 == 0 {
            // SAFETY: the consumed plain Init's edge and initialization proof move together.
            let allocation = unsafe { crate::buffer::raw::Rc::<()>::from_raw(pointer.cast()) };
            let bytes: storage::Shared = Init { allocation }.into_buf().into();
            bytes.into()
        } else {
            // SAFETY: the consumed resident Init retains its complete extent and policy lifetime.
            let allocation =
                unsafe { crate::buffer::raw::Rc::<resident::Lease<'d>>::from_raw(pointer.cast()) };
            Init { allocation }.into_buf().into()
        }
    }

    pub(in crate::buffer) fn bytes(&self) -> usize {
        self.prefix().word().get() as usize
    }

    fn pointer(&self) -> ptr::NonNull<regions::Prefix<num::NonZero<u32>>> {
        // SAFETY: both constructors own aligned nonzero headers; only bit zero is encoded.
        self.pointer
            .map_addr(|address| unsafe { num::NonZero::new_unchecked(address.get() & !1) })
    }

    fn prefix(&self) -> &regions::Prefix<num::NonZero<u32>> {
        // SAFETY: both complete allocation policies begin with a live Prefix.
        unsafe { self.pointer().as_ref() }
    }
}

impl Complete<'_> {
    pub(in crate::buffer) fn try_clone(&self) -> Result<Self, rc::Error> {
        use crate::mem::raw::Edge as _;

        // SAFETY: this complete owner retains the same initialized allocation during retention.
        unsafe { self.prefix().try_retain() }?;
        Ok(Self {
            pointer: self.pointer,
            ownership: marker::PhantomData,
        })
    }
}

impl<'d> Drop for Complete<'d> {
    fn drop(&mut self) {
        use crate::mem::raw::Edge as _;

        // SAFETY: Drop consumes this complete allocation's exact edge once.
        if unsafe { self.prefix().release() } != rc::Release::Last {
            return;
        }
        let pointer = self.pointer();
        if self.pointer.addr().get() & 1 == 0 {
            // SAFETY: the last plain allocation edge grants exclusive destruction.
            unsafe { crate::buffer::raw::Header::<()>::destroy(pointer.cast()) };
        } else {
            // SAFETY: the last resident allocation edge retains its exact policy lifetime.
            unsafe { crate::buffer::raw::Header::<resident::Lease<'d>>::destroy(pointer.cast()) };
        }
    }
}

const _: () = {
    assert!(align_of::<crate::buffer::raw::Header<()>>() > 1);
    assert!(align_of::<crate::buffer::raw::Header<resident::Lease<'static>>>() > 1);
    assert!(size_of::<Complete<'static>>() == size_of::<ptr::NonNull<()>>());
    assert!(size_of::<Option<Complete<'static>>>() == size_of::<ptr::NonNull<()>>());
};
