use std::{cell, marker, mem};

use arena::Slot;
use provided::view;

use crate::{
    buffer,
    buffer::storage::{provided, provided::arena},
    mem::lease,
};

pub struct Vacant<'d, T, L = cell::Cell<u32>, C: Copy = ()> {
    pub(super) node: crate::mem::Lease<arena::Empty<'d>, lease::Open>,
    pub(super) domain: marker::PhantomData<&'d Slot<'d, T, L>>,
    pub(super) context: C,
}

impl<'d, T, L, C: Copy> Vacant<'d, T, L, C> {
    fn entry(&self) -> &'d Slot<'d, T, L> {
        // SAFETY: domain extraction retains the exact slot allocation for 'd.
        unsafe { &*self.node.sink().entry.cast::<Slot<'d, T, L>>().as_ptr() }
    }

    fn insert_admitted<'id>(
        self,
        item: T,
        project: impl FnOnce(&'d mut T) -> buffer::raw::Projection<'d, 'id>,
    ) -> provided::Unique<'d> {
        let slot = self.entry();
        let (node, ()) = self.node.into_parts();
        // SAFETY: the detached vacancy exclusively owns this final item address for 'd.
        let item = unsafe { (*slot.item.get()).write(item) };
        arena::Ready { entry: node.entry }.publish(project(item))
    }
}

impl<'d, T> Vacant<'d, T> {
    pub fn try_insert(
        self,
        item: T,
        resident: u32,
        project: impl for<'a> FnOnce(&'a mut T) -> &'a mut [u8],
    ) -> Result<provided::Unique<'d>, crate::mem::Rejected<T, view::LengthError>> {
        let slot = self.entry();
        // SAFETY: this vacancy exclusively owns the detached slot throughout projection.
        let item = unsafe { (*slot.item.get()).write(item) };
        buffer::raw::Limit::scope(resident, |limit| {
            let view = match limit.try_project(project(item)) {
                Ok(view) => view,
                Err(error) => {
                    // SAFETY: the rejected projection ended its borrow. Move the item
                    // back before the vacancy returns its original slot to the free list.
                    let item = unsafe { (*slot.item.get()).assume_init_read() };
                    return Err(crate::mem::Rejected::new(item, error));
                }
            };
            slot.set_resident(resident);
            let (node, ()) = self.node.into_parts();
            Ok(arena::Ready { entry: node.entry }.publish(view))
        })
    }

    pub fn try_adopt<P: buffer::raw::Input<T>>(
        self,
        item: P,
        resident: u32,
    ) -> Result<provided::Unique<'d>, crate::mem::Rejected<P, view::LengthError>> {
        let len = item.bytes().len();
        if len > resident as usize {
            return Err(crate::mem::Rejected::new(
                item,
                view::LengthError::new(len, resident),
            ));
        }
        let (item, project) = item.into_item();
        self.entry().set_resident(resident);
        Ok(buffer::raw::Limit::scope(resident, |limit| {
            // SAFETY: Input preserves the checked extent through conversion and projection.
            self.insert_admitted(item, |item| unsafe { limit.project(project(item)) })
        }))
    }
}

impl<'d, T, C> Vacant<'d, T, (), &'d C> {
    pub fn context(&self) -> &'d C {
        self.context
    }

    /// # Safety
    /// Own this context's completed item with its initialized prefix and exact fixed layout.
    pub(in crate::buffer) unsafe fn insert_fixed(self, item: T) -> provided::Unique<'d>
    where
        C: buffer::raw::Fixed<T>,
    {
        // SAFETY: the fixed arena initialized every slot with its admitted resident ceiling.
        let resident = unsafe { arena::Entry::resident(self.node.sink().entry) };
        let context = self.context;
        buffer::raw::Limit::scope(resident, |limit| {
            // SAFETY: the native admission and this vacancy retain the same context and layout.
            self.insert_admitted(item, |item| unsafe { context.project(item, limit) })
        })
    }
}

const _: () = {
    assert!(mem::offset_of!(Slot<'_, u8, ()>, item) == size_of::<arena::Entry<'_>>());
    assert!(mem::offset_of!(Slot<'_, u64, cell::Cell<u32>>, item) == size_of::<arena::Entry<'_>>());
    assert!(size_of::<Vacant<'static, ()>>() == size_of::<usize>());
    assert!(size_of::<Vacant<'static, (), (), &'static ()>>() == 2 * size_of::<usize>());
};
