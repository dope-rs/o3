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

impl<T> Header<T> {
    pub(in crate::buffer) fn count(&self) -> &mem::raw::Count {
        &self.refs
    }

    /// # Safety
    /// The terminal edge excludes every chain and borrowed header.
    pub(in crate::buffer) unsafe fn destroy(mut pointer: ptr::NonNull<Self>) {
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
