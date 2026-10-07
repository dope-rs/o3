use std::{ptr, slice};

use crate::{
    alloc,
    depot::{arena::linked::slots, capacity, queue::links},
};

/// Owns the linked-slot region and every initialized lane in its allocation.
pub(in crate::depot) struct QueueBlock<T, S = (), C: crate::depot::raw::Count = capacity::Count> {
    pub(in crate::depot) store: slots::Store<T>,
    pub(in crate::depot) slots: ptr::NonNull<crate::depot::raw::QueueSlot<S>>,
    lanes: C,
}

impl<T, S, C: crate::depot::raw::Count> QueueBlock<T, S, C> {
    pub(in crate::depot) fn try_with(
        count: capacity::Count,
        lanes: C,
        mut init: impl FnMut(links::Index) -> S,
    ) -> Result<Self, alloc::Error> {
        let capacity = slots::Capacity::new(count.get())?;
        let counted = lanes.count();
        let bundle = capacity
            .bundle()
            .try_tail::<crate::depot::raw::QueueSlot<S>>(counted.get())?;
        let fields = bundle.try_allocate()?;
        for index in 0..counted.raw() {
            // SAFETY: the exclusive u32 count keeps every lane below the END sentinel.
            let index = unsafe { links::Index::new_usize(index as usize) };
            let state = init(index);
            // SAFETY: every lane is initialized once before this owner is published.
            unsafe {
                fields
                    .tail()
                    .add(index.get())
                    .write(crate::depot::raw::QueueSlot {
                        queue: slots::State::new(),
                        state,
                    })
            };
        }
        Ok(Self {
            // SAFETY: the bundle's first region has this exact slot capacity.
            store: unsafe { slots::Store::new(fields.prior().root(), capacity) },
            slots: fields.tail(),
            lanes,
        })
    }
    pub(in crate::depot) const fn count(&self) -> C {
        self.lanes
    }

    pub(in crate::depot) fn slots(&self) -> &[crate::depot::raw::QueueSlot<S>] {
        // SAFETY: this owner initializes every lane before publication and retains its extent.
        unsafe { slice::from_raw_parts(self.slots.as_ptr(), self.lanes.count().get()) }
    }

    pub(in crate::depot) fn split(
        &mut self,
    ) -> (&slots::Store<T>, &mut [crate::depot::raw::QueueSlot<S>]) {
        // SAFETY: the exact initialized tail is disjoint from the store.
        let slots =
            unsafe { slice::from_raw_parts_mut(self.slots.as_ptr(), self.lanes.count().get()) };
        (&self.store, slots)
    }
}

impl<T, S, C: crate::depot::raw::Count> Drop for QueueBlock<T, S, C> {
    fn drop(&mut self) {
        // SAFETY: terminal ownership excludes every loan into either initialized region.
        unsafe { self.store.drop_values() };
        let slots = ptr::slice_from_raw_parts_mut(self.slots.as_ptr(), self.lanes.count().get());
        unsafe { ptr::drop_in_place(slots) };
        let bundle = self.store.region.capacity().bundle();
        // SAFETY: the immutable count reconstructs the allocation's original tail.
        let bundle =
            unsafe { bundle.tail::<crate::depot::raw::QueueSlot<S>>(self.lanes.count().get()) };
        unsafe { bundle.deallocate(self.store.region.pointer()) };
    }
}
