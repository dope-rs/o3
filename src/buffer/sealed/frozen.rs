use std::{alloc, marker, ptr, slice};

use crate::buffer::sealed::core;

pub struct Frozen {
    core: ptr::NonNull<core::Core>,
    index: u32,
    len: u32,
    marker: marker::PhantomData<*mut ()>,
}

impl Frozen {
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

impl Clone for Frozen {
    fn clone(&self) -> Self {
        let slot = unsafe { &*core::Core::slot(self.core, self.index) };
        slot.refs.retain();
        Self::new(self.core, self.index, self.len)
    }
}

impl AsRef<[u8]> for Frozen {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        let core = unsafe { self.core.as_ref() };
        let slot = unsafe { &*core::Core::slot(self.core, self.index) };
        if !slot.refs.release() {
            return;
        }
        slot.refs.deactivate();
        slot.next.set(core.free.get());
        core.free.set(self.index);
        core.free_len.set(core.free_len.get() + 1);
        if !core.refs.release() {
            return;
        }
        let layout = unsafe {
            alloc::Layout::from_size_align_unchecked(core.allocation_size, align_of::<core::Core>())
        };
        unsafe { alloc::dealloc(self.core.as_ptr().cast(), layout) };
    }
}
