use std::{mem::size_of, ptr};

use crate::{
    buffer,
    buffer::storage::{provided::arena, regions},
};

/// One live external edge branded by its arena allocation.
#[derive(Debug)]
pub struct Owner<'d> {
    pub(super) entry: ptr::NonNull<arena::Entry<'d>>,
}

const _: () = assert!(size_of::<Owner<'static>>() == size_of::<usize>());

impl regions::End for Owner<'_> {
    const EMPTY: Option<Self> = None;
}

impl buffer::Residency for Owner<'_> {
    fn resident(&self) -> usize {
        // SAFETY: the external edge retains its entire original slot allocation.
        (unsafe { arena::Entry::resident(self.entry) }) as usize
    }
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        // SAFETY: Drop consumes this owner's sole live external edge exactly once.
        unsafe { arena::Entry::release(self) };
    }
}
