use std::{alloc::Layout, ptr};

use crate::{
    alloc,
    buffer::{
        segments,
        segments::{Handle, handles::Descriptor},
    },
    depot::arena::linked::slots,
    mem,
};

#[derive(Debug)]
pub struct Header<T> {
    pub(super) allocation: Layout,
    pub(super) refs: mem::raw::Count,
    pub(super) nodes: slots::Store<segments::Item<T>>,
    pub(super) descriptors: slots::Store<Descriptor>,
}

// SAFETY: every published header owns exactly this reference domain.
unsafe impl<T> mem::raw::Edge for Header<T> {
    fn count(&self) -> &mem::raw::Count {
        &self.refs
    }
}

// SAFETY: the header owns the exact bundled node and descriptor allocations.
unsafe impl<T> mem::raw::Target for Header<T> {
    unsafe fn destroy(mut pointer: ptr::NonNull<Self>) {
        // SAFETY: the terminal edge excludes every chain and borrowed header.
        unsafe {
            let header = pointer.as_mut();
            header.nodes.drop_values();
            header.descriptors.drop_values();
            let layout = header.allocation;
            alloc::raw::LayoutExt::deallocate(layout, pointer);
        }
    }
}

impl<T> Handle<T> for &Header<T> {
    fn header(&self) -> &segments::Header<T> {
        self
    }
}
