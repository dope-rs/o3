use std::{cell, ptr};

use pool::state;

use crate::buffer::{self, pool};

const NONE: u32 = u32::MAX;

#[repr(C)]
pub(super) struct Core {
    pub(super) refs: crate::cell::LocalRefCount,
    pub(super) free: cell::Cell<u32>,
    pub(super) free_len: cell::Cell<u32>,
    pub(super) slots: u32,
    pub(super) capacity: u32,
    pub(super) data_offset: usize,
    pub(super) allocation_size: usize,
}

#[repr(C)]
pub(super) struct Slot {
    pub(super) refs: crate::cell::LocalRefCount,
    pub(super) next: cell::Cell<u32>,
}

const _: () = assert!(align_of::<Core>() >= align_of::<Slot>());

impl Core {
    pub(super) fn allocate<S: state::State>(layout: buffer::Layout) -> ptr::NonNull<Self> {
        match Self::try_allocate::<S>(layout) {
            Ok(core) => core,
            Err(_) => {
                use std::alloc::handle_alloc_error;
                handle_alloc_error(layout.allocation())
            }
        }
    }

    pub(super) fn try_allocate<S: state::State>(
        layout: buffer::Layout,
    ) -> Result<ptr::NonNull<Self>, pool::AllocationError> {
        let raw = if S::ZEROED {
            unsafe {
                use std::alloc::alloc_zeroed;
                alloc_zeroed(layout.allocation())
            }
        } else {
            unsafe {
                use std::alloc::alloc;
                alloc(layout.allocation())
            }
        };
        let ptr = ptr::NonNull::new(raw.cast::<Self>()).ok_or(pool::AllocationError)?;
        unsafe {
            ptr.write(Self {
                refs: crate::cell::LocalRefCount::one(),
                free: cell::Cell::new(if layout.slot_count() == 0 { NONE } else { 0 }),
                free_len: cell::Cell::new(layout.slot_count()),
                slots: layout.slot_count(),
                capacity: layout.capacity().get(),
                data_offset: layout.data_offset(),
                allocation_size: layout.allocation().size(),
            });
            let slot_ptr = ptr
                .as_ptr()
                .cast::<u8>()
                .add(size_of::<Self>())
                .cast::<Slot>();
            for index in 0..layout.slot_count() {
                slot_ptr.add(index as usize).write(Slot {
                    refs: crate::cell::LocalRefCount::empty(),
                    next: cell::Cell::new(if index + 1 == layout.slot_count() {
                        NONE
                    } else {
                        index + 1
                    }),
                });
            }
        }
        Ok(ptr)
    }

    pub(super) fn acquire_owned(ptr: ptr::NonNull<Self>) -> Option<u32> {
        let index = Self::acquire_borrowed(ptr)?;
        unsafe { ptr.as_ref() }.refs.retain();
        Some(index)
    }

    pub(super) fn acquire_borrowed(ptr: ptr::NonNull<Self>) -> Option<u32> {
        let core = unsafe { ptr.as_ref() };
        let index = core.free.get();
        if index == NONE {
            return None;
        }
        let slot = unsafe { &*Self::slot(ptr, index) };
        debug_assert!(slot.refs.is_empty());
        core.free.set(slot.next.get());
        core.free_len.set(core.free_len.get() - 1);
        slot.refs.activate();
        Some(index)
    }

    pub(super) fn slot(ptr: ptr::NonNull<Self>, index: u32) -> *mut Slot {
        let core = unsafe { ptr.as_ref() };
        debug_assert!(index < core.slots);
        unsafe {
            ptr.as_ptr()
                .cast::<u8>()
                .add(size_of::<Self>())
                .cast::<Slot>()
                .add(index as usize)
        }
    }

    pub(super) fn data(ptr: ptr::NonNull<Self>, index: u32) -> *mut u8 {
        let core = unsafe { ptr.as_ref() };
        debug_assert!(index < core.slots);
        unsafe {
            ptr.as_ptr()
                .cast::<u8>()
                .add(core.data_offset + index as usize * core.capacity as usize)
        }
    }
}
