use std::{cell, marker, mem};

use arena::{Returned, Slot};
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
    pub(in crate::buffer::storage::provided) fn entry(&self) -> &'d Slot<'d, T, L> {
        // SAFETY: domain extraction retains the exact slot allocation for 'd.
        unsafe { &*self.node.sink().entry.cast::<Slot<'d, T, L>>().as_ptr() }
    }

    pub(in crate::buffer) fn insert_with(
        self,
        item: T,
        project: impl for<'id> FnOnce(
            &'d mut T,
            buffer::raw::Limit<'id>,
        ) -> buffer::raw::Projection<'d, 'id>,
    ) -> provided::Unique<'d> {
        let pending = self.initialize(item);
        // SAFETY: the pending owner retains this complete initialized slot and its layout.
        let resident = unsafe { arena::Entry::resident(pending.node.entry) };
        // SAFETY: the affine initialized slot and its allocation remain owned
        // through publication; only this projection can access its item.
        let slot = pending.entry();
        let item = unsafe { (*slot.item.get()).assume_init_mut() };
        buffer::raw::Limit::scope(resident, |limit| pending.publish(project(item, limit)))
    }

    fn initialize(self, item: T) -> Returned<'d, T, L> {
        // SAFETY: the vacancy exclusively owns one uninitialized detached item.
        unsafe { (*self.entry().item.get()).write(item) };
        let (node, ()) = self.node.into_parts();
        Returned {
            node: arena::Entry::initialize(node),
            domain: marker::PhantomData,
        }
    }
}

const _: () = {
    assert!(mem::offset_of!(Slot<'_, u8, ()>, item) == usize::BITS as usize);
    assert!(mem::offset_of!(Slot<'_, u64, cell::Cell<u32>>, item) == 9 * size_of::<usize>());
};

impl<'d, T> Vacant<'d, T> {
    pub fn try_insert(
        self,
        item: T,
        resident: u32,
        project: impl for<'a> FnOnce(&'a mut T) -> &'a mut [u8],
    ) -> Result<provided::Unique<'d>, crate::mem::Rejected<T, view::LengthError>> {
        self.entry().set_resident(resident);
        let pending = self.initialize(item);
        buffer::raw::Limit::scope(resident, |limit| {
            // SAFETY: this callback borrows the initialized item only for
            // projection; rejection moves it after that borrow ends.
            let slot = pending.entry();
            let item = unsafe { (*slot.item.get()).assume_init_mut() };
            let view = match limit.try_project(project(item)) {
                Ok(view) => view,
                Err(error) => return Err(pending.reject(error)),
            };
            Ok(pending.publish(view))
        })
    }
    pub fn try_adopt<P: buffer::raw::Input<T>>(
        self,
        item: P,
        resident: u32,
    ) -> Result<provided::Unique<'d>, crate::mem::Rejected<P, view::LengthError>> {
        self.entry().set_resident(resident);
        let len = item.bytes().len();
        if len > resident as usize {
            return Err(crate::mem::Rejected::new(
                item,
                view::LengthError::new(len, resident),
            ));
        }
        let (item, project) = item.into_item();
        Ok(self.insert_with(item, |item, limit| {
            // SAFETY: Input retains the original owner and checked projection through conversion.
            unsafe { limit.project(project(item)) }
        }))
    }
}
impl<'d, T, C> Vacant<'d, T, (), &'d C> {
    pub fn context(&self) -> &'d C {
        self.context
    }
}
const _: () = {
    assert!(size_of::<Vacant<'static, ()>>() == size_of::<usize>());
    assert!(size_of::<Vacant<'static, (), (), &'static ()>>() == 2 * size_of::<usize>());
};
