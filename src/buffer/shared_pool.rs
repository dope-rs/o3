use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::cell::Cell;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::slice;

use super::SpareWriter;

const NONE: u32 = u32::MAX;

#[repr(C)]
struct Group {
    refs: Cell<u32>,
    free: Cell<u32>,
    free_len: Cell<u32>,
    slots: u32,
    capacity: u32,
    slot_offset: usize,
    data_offset: usize,
}

#[repr(C)]
struct Slot {
    refs: Cell<u32>,
    next: Cell<u32>,
}

impl Group {
    fn layout(slots: usize, capacity: usize) -> (Layout, usize, usize) {
        let slots_layout = Layout::array::<Slot>(slots).expect("shared pool slot overflow");
        let bytes = slots
            .checked_mul(capacity)
            .expect("shared pool capacity overflow");
        let bytes_layout = Layout::array::<u8>(bytes).expect("shared pool capacity overflow");
        let (layout, slot_offset) = Layout::new::<Group>()
            .extend(slots_layout)
            .expect("shared pool layout overflow");
        let (layout, data_offset) = layout
            .extend(bytes_layout)
            .expect("shared pool layout overflow");
        (layout.pad_to_align(), slot_offset, data_offset)
    }

    fn allocate(slots: usize, capacity: usize) -> NonNull<Self> {
        assert!(capacity > 0, "shared pool needs capacity");
        assert!(u32::try_from(slots).is_ok(), "shared pool slot overflow");
        assert!(
            u32::try_from(capacity).is_ok(),
            "shared pool capacity overflow"
        );
        let (layout, slot_offset, data_offset) = Self::layout(slots, capacity);
        let ptr = NonNull::new(unsafe { alloc(layout) }.cast::<Self>())
            .unwrap_or_else(|| handle_alloc_error(layout));
        unsafe {
            ptr.write(Self {
                refs: Cell::new(1),
                free: Cell::new(if slots == 0 { NONE } else { 0 }),
                free_len: Cell::new(slots as u32),
                slots: slots as u32,
                capacity: capacity as u32,
                slot_offset,
                data_offset,
            });
            let slot_ptr = ptr.as_ptr().cast::<u8>().add(slot_offset).cast::<Slot>();
            for index in 0..slots as u32 {
                slot_ptr.add(index as usize).write(Slot {
                    refs: Cell::new(0),
                    next: Cell::new(if index + 1 == slots as u32 {
                        NONE
                    } else {
                        index + 1
                    }),
                });
            }
        }
        ptr
    }

    unsafe fn retain(ptr: NonNull<Self>) {
        let refs = unsafe { ptr.as_ref() }.refs.get();
        assert!(refs != u32::MAX, "shared pool reference overflow");
        unsafe { ptr.as_ref() }.refs.set(refs + 1);
    }

    unsafe fn release(ptr: NonNull<Self>) {
        let group = unsafe { ptr.as_ref() };
        let refs = group.refs.get();
        debug_assert_ne!(refs, 0);
        if refs != 1 {
            group.refs.set(refs - 1);
            return;
        }
        let (layout, _, _) = Self::layout(group.slots as usize, group.capacity as usize);
        unsafe { dealloc(ptr.as_ptr().cast(), layout) };
    }

    unsafe fn slot(ptr: NonNull<Self>, index: u32) -> *mut Slot {
        let group = unsafe { ptr.as_ref() };
        debug_assert!(index < group.slots);
        unsafe {
            ptr.as_ptr()
                .cast::<u8>()
                .add(group.slot_offset)
                .cast::<Slot>()
                .add(index as usize)
        }
    }

    unsafe fn data(ptr: NonNull<Self>, index: u32) -> *mut u8 {
        let group = unsafe { ptr.as_ref() };
        debug_assert!(index < group.slots);
        unsafe {
            ptr.as_ptr()
                .cast::<u8>()
                .add(group.data_offset + index as usize * group.capacity as usize)
        }
    }

    unsafe fn acquire(ptr: NonNull<Self>) -> Option<u32> {
        let group = unsafe { ptr.as_ref() };
        let index = group.free.get();
        if index == NONE {
            return None;
        }
        let refs = group.refs.get();
        assert!(refs != u32::MAX, "shared pool reference overflow");
        let slot = unsafe { &*Self::slot(ptr, index) };
        debug_assert_eq!(slot.refs.get(), 0);
        group.free.set(slot.next.get());
        group.free_len.set(group.free_len.get() - 1);
        slot.refs.set(1);
        group.refs.set(refs + 1);
        Some(index)
    }

    unsafe fn retain_slot(ptr: NonNull<Self>, index: u32) {
        let slot = unsafe { &*Self::slot(ptr, index) };
        let refs = slot.refs.get();
        assert!(refs != u32::MAX, "pooled buffer reference overflow");
        debug_assert_ne!(refs, 0);
        slot.refs.set(refs + 1);
    }

    unsafe fn release_slot(ptr: NonNull<Self>, index: u32) {
        let group = unsafe { ptr.as_ref() };
        let slot = unsafe { &*Self::slot(ptr, index) };
        let refs = slot.refs.get();
        debug_assert_ne!(refs, 0);
        if refs != 1 {
            slot.refs.set(refs - 1);
            return;
        }
        slot.refs.set(0);
        slot.next.set(group.free.get());
        group.free.set(index);
        group.free_len.set(group.free_len.get() + 1);
        unsafe { Self::release(ptr) };
    }
}

pub struct SharedPool {
    group: NonNull<Group>,
    marker: PhantomData<*mut ()>,
}

impl SharedPool {
    pub fn new(slots: usize, capacity: usize) -> Self {
        Self {
            group: Group::allocate(slots, capacity),
            marker: PhantomData,
        }
    }

    pub fn try_acquire(&self) -> Option<SharedLease> {
        let index = unsafe { Group::acquire(self.group) }?;
        Some(SharedLease {
            group: self.group,
            index,
            len: 0,
            marker: PhantomData,
        })
    }

    pub fn capacity(&self) -> usize {
        unsafe { self.group.as_ref() }.capacity as usize
    }

    pub fn available(&self) -> usize {
        unsafe { self.group.as_ref() }.free_len.get() as usize
    }
}

impl Clone for SharedPool {
    fn clone(&self) -> Self {
        unsafe { Group::retain(self.group) };
        Self {
            group: self.group,
            marker: PhantomData,
        }
    }
}

impl Drop for SharedPool {
    fn drop(&mut self) {
        unsafe { Group::release(self.group) };
    }
}

pub struct SharedLease {
    group: NonNull<Group>,
    index: u32,
    len: u32,
    marker: PhantomData<*mut ()>,
}

macro_rules! impl_shared_access {
    () => {
        pub fn len(&self) -> usize {
            self.len as usize
        }

        pub fn is_empty(&self) -> bool {
            self.len == 0
        }

        pub fn as_slice(&self) -> &[u8] {
            unsafe { slice::from_raw_parts(Group::data(self.group, self.index), self.len as usize) }
        }
    };
}

impl SharedLease {
    impl_shared_access!();

    pub fn capacity(&self) -> usize {
        unsafe { self.group.as_ref() }.capacity as usize
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(Group::data(self.group, self.index), self.len as usize) }
    }

    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        let group = unsafe { self.group.as_ref() };
        let len = self.len as usize;
        let ptr = unsafe { Group::data(self.group, self.index).add(len).cast() };
        unsafe { SpareWriter::new(ptr, group.capacity as usize - len, &mut self.len) }
    }

    pub fn freeze(self) -> Pooled {
        let this = std::mem::ManuallyDrop::new(self);
        Pooled {
            group: this.group,
            index: this.index,
            len: this.len,
            marker: PhantomData,
        }
    }
}

impl Drop for SharedLease {
    fn drop(&mut self) {
        unsafe { Group::release_slot(self.group, self.index) };
    }
}

pub struct Pooled {
    group: NonNull<Group>,
    index: u32,
    len: u32,
    marker: PhantomData<*mut ()>,
}

impl Pooled {
    impl_shared_access!();
}

impl Clone for Pooled {
    fn clone(&self) -> Self {
        unsafe { Group::retain_slot(self.group, self.index) };
        Self {
            group: self.group,
            index: self.index,
            len: self.len,
            marker: PhantomData,
        }
    }
}

impl AsRef<[u8]> for Pooled {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for Pooled {
    fn drop(&mut self) {
        unsafe { Group::release_slot(self.group, self.index) };
    }
}
