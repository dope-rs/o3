use std::{cell, convert, marker, mem, ptr};

use crate::{
    alloc,
    depot::{
        arena::{slab, slab::entries},
        capacity,
        queue::links,
    },
};

#[derive(Clone, Copy)]
pub(in crate::depot) struct SlabLinks {
    pub(in crate::depot) next: u32,
    pub(in crate::depot) prev: u32,
}

union Data<T> {
    links: SlabLinks,
    value: mem::ManuallyDrop<T>,
}

/// One raw slot cell set.
///
/// Declared `pub` (like the other sealed-module items) so the sealed `slab::storage::Plane`
/// bound can name it, while this private module keeps it unnameable outside `depot`.
pub struct SlabSlot<T, G: Copy, B: Copy, R: Copy> {
    pub(in crate::depot) state: cell::Cell<slab::State<B, R>>,
    pub(in crate::depot) generation: cell::Cell<G>,
    pub(in crate::depot) position: cell::Cell<u32>,
    data: cell::UnsafeCell<Data<T>>,
}

impl<T, G: Copy, B: Copy, R: Copy> SlabSlot<T, G, B, R> {
    /// # Safety
    /// This slot is an element of the original `slots`, whose length is at most `u32::MAX`.
    pub(in crate::depot) unsafe fn index(&self, slots: &[Self]) -> links::Index {
        unsafe { links::Index::new_usize(ptr::from_ref(self).offset_from_unsigned(slots.as_ptr())) }
    }

    pub(in crate::depot) fn free(epoch: G, next: u32, prev: u32) -> Self {
        Self {
            state: cell::Cell::new(slab::State::Free),
            generation: cell::Cell::new(epoch),
            position: cell::Cell::new(entries::NONE),
            data: cell::UnsafeCell::new(Data {
                links: SlabLinks { next, prev },
            }),
        }
    }

    pub(in crate::depot) unsafe fn links(&self) -> SlabLinks {
        // SAFETY: caller proves Free/Reserved state selects the SlabLinks union member.
        unsafe { (*self.data.get()).links }
    }

    pub(in crate::depot) unsafe fn set_links(&self, links: SlabLinks) {
        // SAFETY: caller exclusively owns a non-value state before writing SlabLinks.
        unsafe { (*self.data.get()).links = links };
    }

    pub(in crate::depot) unsafe fn set_next(&self, next: u32) {
        // SAFETY: caller proves Free/Reserved state selects the SlabLinks union member.
        unsafe { (*self.data.get()).links.next = next };
    }

    pub(in crate::depot) unsafe fn set_prev(&self, prev: u32) {
        // SAFETY: caller proves Free/Reserved state selects the SlabLinks union member.
        unsafe { (*self.data.get()).links.prev = prev };
    }

    pub(in crate::depot) unsafe fn value(&self) -> &T {
        // SAFETY: caller proves Occupied/Busy state selects an initialized value.
        unsafe { &*(&raw const (*self.data.get()).value).cast::<T>() }
    }

    pub(in crate::depot) unsafe fn value_ptr(&self) -> *mut T {
        // SAFETY: caller proves exclusive access to the initialized value member.
        unsafe { (&raw mut (*self.data.get()).value).cast::<T>() }
    }

    pub(in crate::depot) unsafe fn write_value(&self, value: T) {
        // SAFETY: caller owns Reserved state and initializes the inactive union member once.
        unsafe { (*self.data.get()).value = mem::ManuallyDrop::new(value) };
    }

    pub(in crate::depot) unsafe fn take_value(&self) -> T {
        // SAFETY: caller owns the initialized value and transitions away from Occupied.
        unsafe { mem::ManuallyDrop::take(&mut (*self.data.get()).value) }
    }

    pub(in crate::depot) unsafe fn drop_value(&self) {
        // SAFETY: caller owns the initialized value and transitions away from Occupied.
        unsafe { mem::ManuallyDrop::drop(&mut (*self.data.get()).value) };
    }
}

pub(in crate::depot) struct SlabCapacity<T, G: Copy, B: Copy, R: Copy> {
    pub(in crate::depot) count: capacity::Count,
    slot: Fields<T, G, B, R>,
}

impl<T, G: Copy, B: Copy, R: Copy> SlabCapacity<T, G, B, R> {
    pub(in crate::depot) fn new(count: capacity::Count) -> Result<Self, alloc::Overflow> {
        use alloc::raw::Rows;

        if count.get() > Self::MAX {
            return Err(alloc::Overflow);
        }
        Ok(Self {
            count,
            slot: marker::PhantomData,
        })
    }
}

unsafe impl<T, G: Copy, B: Copy, R: Copy> alloc::raw::Shape for SlabCapacity<T, G, B, R> {
    type Root = SlabSlot<T, G, B, R>;
    type Tail = cell::Cell<mem::MaybeUninit<u32>>;
    type Extra = ();
    type Last = ();

    fn len(&self) -> usize {
        self.count.get()
    }
}

unsafe impl<T, G: Copy, B: Copy, R: Copy> alloc::raw::Rows for SlabCapacity<T, G, B, R> {}

const _: () = {
    assert!(mem::size_of::<slab::State<(), ()>>() == 1);
    assert!(mem::size_of::<slab::State<(), convert::Infallible>>() == 1);
    assert!(mem::size_of::<slab::State<convert::Infallible, ()>>() == 1);
    assert!(mem::size_of::<slab::State<convert::Infallible, convert::Infallible>>() == 1);
    assert!(
        mem::size_of::<SlabSlot<u64, u32, (), ()>>()
            == mem::size_of::<SlabSlot<u64, u32, (), convert::Infallible>>()
    );
    assert!(
        mem::size_of::<SlabSlot<u64, u32, (), ()>>()
            == mem::size_of::<SlabSlot<u64, u32, convert::Infallible, ()>>()
    );
    assert!(
        mem::size_of::<SlabSlot<u64, u32, (), ()>>()
            == mem::size_of::<SlabSlot<u64, u32, convert::Infallible, convert::Infallible>>()
    );
};

type Fields<T, G, B, R> = marker::PhantomData<fn() -> SlabSlot<T, G, B, R>>;
