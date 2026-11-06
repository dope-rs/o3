use std::ptr;

use crate::{buffer::resident::staging, mem};

// SAFETY: the staging count belongs to this exact boxed state.
unsafe impl mem::raw::Edge for staging::Header<'_> {
    fn count(&self) -> &mem::raw::Count {
        &self.refs
    }
}

// SAFETY: the staging state was allocated as one Box and retains its counted edges.
unsafe impl mem::raw::Target for staging::Header<'_> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: the terminal edge transfers the exact initialized Box exclusively.
        drop(unsafe { Box::from_raw(pointer.as_ptr()) });
    }
}
