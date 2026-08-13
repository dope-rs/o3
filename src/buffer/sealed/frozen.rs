use std::{alloc, marker, mem, ptr, slice};

use crate::buffer::{pool, sealed::core};

pub struct Frozen<O: pool::Ownership = pool::Owned> {
    core: ptr::NonNull<core::Core>,
    index: u32,
    len: u32,
    marker: marker::PhantomData<(O, *mut ())>,
}

impl<O: pool::Ownership> Frozen<O> {
    pub(super) fn new(core: ptr::NonNull<core::Core>, index: u32, len: u32) -> Self {
        Self {
            core,
            index,
            len,
            marker: marker::PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn capacity(&self) -> usize {
        unsafe { self.core.as_ref() }.capacity as usize
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(core::Core::data(self.core, self.index), self.len()) }
    }
}

impl Frozen<pool::Borrowed<'_>> {
    /// Erases the pool borrow by retaining the allocation exactly once.
    pub fn into_owned(self) -> Frozen {
        unsafe { self.core.as_ref() }.refs.retain();
        let this = mem::ManuallyDrop::new(self);
        Frozen::new(this.core, this.index, this.len)
    }
}

impl<O: pool::Ownership> Clone for Frozen<O> {
    fn clone(&self) -> Self {
        if O::RETAINS_CORE {
            unsafe { self.core.as_ref() }.refs.retain();
        }
        let slot = unsafe { &*core::Core::slot(self.core, self.index) };
        slot.refs.retain();
        Self::new(self.core, self.index, self.len)
    }
}

impl<O: pool::Ownership> AsRef<[u8]> for Frozen<O> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<O: pool::Ownership> Drop for Frozen<O> {
    fn drop(&mut self) {
        let core = unsafe { self.core.as_ref() };
        let slot = unsafe { &*core::Core::slot(self.core, self.index) };
        if slot.refs.release() {
            slot.refs.deactivate();
            slot.next.set(core.free.get());
            core.free.set(self.index);
            core.free_len.set(core.free_len.get() + 1);
        }
        if O::RETAINS_CORE && core.refs.release() {
            let layout = unsafe {
                alloc::Layout::from_size_align_unchecked(
                    core.allocation_size,
                    align_of::<core::Core>(),
                )
            };
            unsafe { alloc::dealloc(self.core.as_ptr().cast(), layout) };
        }
    }
}
