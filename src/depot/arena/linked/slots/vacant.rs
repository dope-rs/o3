use std::{marker, mem, ptr};

use crate::depot::arena::{linked, linked::slots};

/// Sole return entitlement, retaining the allocation's original owner.
#[must_use = "a vacant slot retains its sole return entitlement"]
pub struct Vacant<T, H: linked::Source<T>, Tag = ()> {
    pub(super) owner: H,
    pub(super) index: slots::Index<Tag>,
    pub(super) value: marker::PhantomData<T>,
}

impl<T, H: linked::Source<T>, Tag> Vacant<T, H, Tag> {
    /// # Safety
    /// This owner's store counts all available slots and the owner refunds each claim.
    pub(in crate::depot) unsafe fn try_reserve(owner: H) -> Option<Self> {
        if owner.store().available() == 0 {
            return None;
        }
        Some(unsafe { Self::reserve(owner) })
    }

    /// # Safety
    /// This exact owner has one available slot.
    pub(crate) unsafe fn reserve(owner: H) -> Self {
        let store = owner.store();
        if H::TRACK {
            store.available.set(store.available.get().wrapping_sub(1));
        }
        unsafe { Self::claim(owner) }
    }

    /// # Safety
    /// One already debited reservation is consumed from this exact store.
    pub(in crate::depot::arena::linked) unsafe fn claim(owner: H) -> Self {
        let (index, _) = unsafe { owner.store().claim::<false>() };
        Self {
            owner,
            index: slots::Index::new(index),
            value: marker::PhantomData,
        }
    }

    /// # Safety
    /// This exact owner has one available slot.
    pub(crate) unsafe fn insert_one(owner: H, value: T) -> slots::Live<T, H, Tag> {
        let store = owner.store();
        if H::TRACK {
            store.available.set(store.available.get().wrapping_sub(1));
        }
        let (index, node) = unsafe { store.claim::<true>() };
        unsafe { node.value.get().cast::<T>().write(value) };
        unsafe { slots::Live::detached(owner, slots::Index::new(index)) }
    }

    pub fn insert(self, value: T) -> slots::Live<T, H, Tag> {
        let node = unsafe { self.owner.store().slot(self.index.raw()) };
        unsafe { node.value.get().cast::<T>().write(value) };
        node.next.set(slots::Store::<T>::END);
        slots::Live { vacant: self }
    }

    pub(crate) fn release(self) -> H {
        let owner = self.recycle();
        if H::TRACK {
            owner.store().refund(1);
        }
        owner
    }

    pub(in crate::depot::arena::linked) fn recycle(self) -> H {
        let this = mem::ManuallyDrop::new(self);
        let store = this.owner.store();
        let index = this.index.raw();
        let node = unsafe { store.slot(index) };
        store.recycle(index, node);
        unsafe { ptr::read(&this.owner) }
    }
}

impl<T, H: linked::Source<T>, Tag> Drop for Vacant<T, H, Tag> {
    fn drop(&mut self) {
        Vacant {
            owner: &self.owner,
            index: self.index,
            value: marker::PhantomData,
        }
        .release();
    }
}

impl<T, Tag, const TRACK: bool> crate::mem::Return
    for slots::Vacant<T, slots::Bound<'_, T, Tag, TRACK>, Tag>
{
    type Item = T;

    fn return_item(self, item: T) {
        self.insert(item).prepend();
    }
}
