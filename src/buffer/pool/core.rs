use std::{cell, ptr, slice};

use crate::buffer::{
    self,
    pool::{self, state},
    storage::raw::refs,
    write,
};

const NONE: u32 = u32::MAX;

#[repr(C)]
pub(super) struct Core {
    refs: refs::LocalRefCount,
    free: cell::Cell<u32>,
    free_len: cell::Cell<u32>,
    slots: u32,
    capacity: u32,
    data_offset: usize,
    allocation_size: usize,
}

#[repr(C)]
pub(super) struct Slot {
    refs: refs::LocalRefCount,
    next: cell::Cell<u32>,
}

const _: () = assert!(align_of::<Core>() >= align_of::<Slot>());

impl Core {
    pub(super) fn allocate<S: state::State>(layout: pool::Layout) -> ptr::NonNull<Self> {
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
        let ptr = ptr::NonNull::new(raw.cast::<Self>()).unwrap_or_else(|| {
            use std::alloc::handle_alloc_error;
            handle_alloc_error(layout.allocation())
        });
        unsafe {
            use crate::buffer::storage::raw::refs::LocalRefCount;
            ptr.write(Self {
                refs: LocalRefCount::one(),
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
                    refs: LocalRefCount::empty(),
                    next: cell::Cell::new(if index + 1 == layout.slot_count() {
                        NONE
                    } else {
                        index + 1
                    }),
                });
            }
        }
        ptr
    }

    pub(super) fn retain(ptr: ptr::NonNull<Self>) {
        unsafe { ptr.as_ref() }.refs.retain();
    }

    pub(super) fn release(ptr: ptr::NonNull<Self>) {
        let core = unsafe { ptr.as_ref() };
        if !core.refs.release() {
            return;
        }
        let layout = unsafe {
            use std::alloc::Layout;
            Layout::from_size_align_unchecked(core.allocation_size, align_of::<Self>())
        };
        unsafe {
            use std::alloc::dealloc;
            dealloc(ptr.as_ptr().cast(), layout);
        }
    }

    pub(super) fn capacity(ptr: ptr::NonNull<Self>) -> usize {
        unsafe { ptr.as_ref() }.capacity as usize
    }

    pub(super) fn available(ptr: ptr::NonNull<Self>) -> usize {
        unsafe { ptr.as_ref() }.free_len.get() as usize
    }

    pub(super) fn acquire(ptr: ptr::NonNull<Self>) -> Option<u32> {
        let core = unsafe { ptr.as_ref() };
        let index = core.free.get();
        if index == NONE {
            return None;
        }
        core.refs.retain();
        let slot = unsafe { &*Self::slot(ptr, index) };
        debug_assert!(slot.refs.is_empty());
        core.free.set(slot.next.get());
        core.free_len.set(core.free_len.get() - 1);
        slot.refs.activate();
        Some(index)
    }

    pub(super) fn retain_slot(ptr: ptr::NonNull<Self>, index: u32) {
        let slot = unsafe { &*Self::slot(ptr, index) };
        slot.refs.retain();
    }

    pub(super) fn release_slot(ptr: ptr::NonNull<Self>, index: u32) {
        let core = unsafe { ptr.as_ref() };
        let slot = unsafe { &*Self::slot(ptr, index) };
        if !slot.refs.release() {
            return;
        }
        slot.refs.deactivate();
        slot.next.set(core.free.get());
        core.free.set(index);
        core.free_len.set(core.free_len.get() + 1);
        Self::release(ptr);
    }

    pub(super) fn slice<'a>(ptr: ptr::NonNull<Self>, index: u32, len: usize) -> &'a [u8] {
        unsafe { slice::from_raw_parts(Self::data(ptr, index), len) }
    }

    pub(super) fn slice_mut<'a>(ptr: ptr::NonNull<Self>, index: u32, len: usize) -> &'a mut [u8] {
        unsafe { slice::from_raw_parts_mut(Self::data(ptr, index), len) }
    }

    pub(super) fn push(
        ptr: ptr::NonNull<Self>,
        index: u32,
        len: &mut u32,
        byte: u8,
    ) -> Result<(), buffer::CapacityError> {
        let written = *len as usize;
        let capacity = Self::capacity(ptr);
        if written == capacity {
            return Err(buffer::CapacityError::new(
                written.saturating_add(1),
                capacity,
            ));
        }
        unsafe {
            use std::mem::MaybeUninit;
            Self::data(ptr, index)
                .add(written)
                .cast::<MaybeUninit<u8>>()
                .write(MaybeUninit::new(byte));
        }
        *len += 1;
        Ok(())
    }

    pub(super) fn extend(
        ptr: ptr::NonNull<Self>,
        index: u32,
        len: &mut u32,
        src: &[u8],
    ) -> Result<(), buffer::CapacityError> {
        let start = *len as usize;
        let capacity = Self::capacity(ptr);
        let end = start
            .checked_add(src.len())
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(buffer::CapacityError::new(end, capacity));
        }
        unsafe {
            ptr::copy_nonoverlapping(src.as_ptr(), Self::data(ptr, index).add(start), src.len())
        };
        *len = end as u32;
        Ok(())
    }

    pub(super) fn extend_from_slices<const N: usize>(
        ptr: ptr::NonNull<Self>,
        index: u32,
        len: &mut u32,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        use crate::buffer::checked_append_len;
        let start = *len as usize;
        let end = checked_append_len(start, Self::capacity(ptr), &slices)?;
        let mut offset = start;
        for src in slices {
            unsafe {
                ptr::copy_nonoverlapping(
                    src.as_ptr(),
                    Self::data(ptr, index).add(offset),
                    src.len(),
                );
            }
            offset += src.len();
        }
        *len = end as u32;
        Ok(())
    }

    pub(super) fn spare_writer<'a>(
        ptr: ptr::NonNull<Self>,
        index: u32,
        len: &'a mut u32,
    ) -> write::SpareWriter<'a> {
        let capacity = Self::capacity(ptr);
        let written = *len as usize;
        let data = unsafe { Self::data(ptr, index).add(written).cast() };
        unsafe {
            use crate::buffer::write::SpareWriter;
            SpareWriter::new(data, capacity - written, len)
        }
    }

    fn slot(ptr: ptr::NonNull<Self>, index: u32) -> *mut Slot {
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

    fn data(ptr: ptr::NonNull<Self>, index: u32) -> *mut u8 {
        let core = unsafe { ptr.as_ref() };
        debug_assert!(index < core.slots);
        unsafe {
            ptr.as_ptr()
                .cast::<u8>()
                .add(core.data_offset + index as usize * core.capacity as usize)
        }
    }
}
