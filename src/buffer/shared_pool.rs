use std::alloc::{Layout, alloc, alloc_zeroed, dealloc, handle_alloc_error};
use std::cell::Cell;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::slice;

use super::ref_count::LocalRefCount;
use super::{PoolLayoutError, SpareWriter};

const NONE: u32 = u32::MAX;

#[repr(C)]
struct Group {
    refs: LocalRefCount,
    free: Cell<u32>,
    free_len: Cell<u32>,
    slots: u32,
    capacity: u32,
    data_offset: usize,
    allocation_size: usize,
}

#[repr(C)]
struct Slot {
    refs: LocalRefCount,
    next: Cell<u32>,
}

const _: () = assert!(align_of::<Group>() >= align_of::<Slot>());

struct GroupLayout {
    allocation: Layout,
    slots: u32,
    capacity: NonZeroU32,
    data_offset: usize,
}

impl GroupLayout {
    fn new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        let slots = u32::try_from(slots).map_err(|_| PoolLayoutError::SlotOverflow)?;
        let capacity = u32::try_from(capacity)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(if capacity == 0 {
                PoolLayoutError::ZeroCapacity
            } else {
                PoolLayoutError::CapacityOverflow
            })?;
        let slots_layout =
            Layout::array::<Slot>(slots as usize).map_err(|_| PoolLayoutError::CapacityOverflow)?;
        let data_len = (slots as usize)
            .checked_mul(capacity.get() as usize)
            .ok_or(PoolLayoutError::CapacityOverflow)?;
        let data_layout =
            Layout::array::<u8>(data_len).map_err(|_| PoolLayoutError::CapacityOverflow)?;
        let (layout, _) = Layout::new::<Group>()
            .extend(slots_layout)
            .map_err(|_| PoolLayoutError::CapacityOverflow)?;
        let (layout, data_offset) = layout
            .extend(data_layout)
            .map_err(|_| PoolLayoutError::CapacityOverflow)?;
        Ok(Self {
            allocation: layout.pad_to_align(),
            slots,
            capacity,
            data_offset,
        })
    }
}

impl Group {
    fn allocate(layout: GroupLayout, initialized: bool) -> NonNull<Self> {
        let raw = if initialized {
            unsafe { alloc_zeroed(layout.allocation) }
        } else {
            unsafe { alloc(layout.allocation) }
        };
        let ptr = NonNull::new(raw.cast::<Self>())
            .unwrap_or_else(|| handle_alloc_error(layout.allocation));
        unsafe {
            ptr.write(Self {
                refs: LocalRefCount::one(),
                free: Cell::new(if layout.slots == 0 { NONE } else { 0 }),
                free_len: Cell::new(layout.slots),
                slots: layout.slots,
                capacity: layout.capacity.get(),
                data_offset: layout.data_offset,
                allocation_size: layout.allocation.size(),
            });
            let slot_ptr = ptr
                .as_ptr()
                .cast::<u8>()
                .add(size_of::<Group>())
                .cast::<Slot>();
            for index in 0..layout.slots {
                slot_ptr.add(index as usize).write(Slot {
                    refs: LocalRefCount::empty(),
                    next: Cell::new(if index + 1 == layout.slots {
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
        unsafe { ptr.as_ref() }.refs.retain();
    }

    unsafe fn release(ptr: NonNull<Self>) {
        let group = unsafe { ptr.as_ref() };
        if !group.refs.release() {
            return;
        }
        let layout = unsafe {
            Layout::from_size_align_unchecked(group.allocation_size, align_of::<Group>())
        };
        unsafe { dealloc(ptr.as_ptr().cast(), layout) };
    }

    unsafe fn slot(ptr: NonNull<Self>, index: u32) -> *mut Slot {
        let group = unsafe { ptr.as_ref() };
        debug_assert!(index < group.slots);
        unsafe {
            ptr.as_ptr()
                .cast::<u8>()
                .add(size_of::<Group>())
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
        group.refs.retain();
        let slot = unsafe { &*Self::slot(ptr, index) };
        debug_assert!(slot.refs.is_empty());
        group.free.set(slot.next.get());
        group.free_len.set(group.free_len.get() - 1);
        slot.refs.activate();
        Some(index)
    }

    unsafe fn retain_slot(ptr: NonNull<Self>, index: u32) {
        let slot = unsafe { &*Self::slot(ptr, index) };
        slot.refs.retain();
    }

    unsafe fn release_slot(ptr: NonNull<Self>, index: u32) {
        let group = unsafe { ptr.as_ref() };
        let slot = unsafe { &*Self::slot(ptr, index) };
        if !slot.refs.release() {
            return;
        }
        slot.refs.deactivate();
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
    /// Creates a pool, returning an error when its fixed allocation cannot be
    /// represented by the pool layout.
    pub fn try_new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        let layout = GroupLayout::new(slots, capacity)?;
        Ok(Self {
            group: Group::allocate(layout, false),
            marker: PhantomData,
        })
    }

    /// Creates a pool with the requested fixed layout.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero or the requested allocation cannot be
    /// represented. Use [`SharedPool::try_new`] for runtime configuration.
    #[track_caller]
    pub fn new(slots: usize, capacity: usize) -> Self {
        match Self::try_new(slots, capacity) {
            Ok(pool) => pool,
            Err(error) => panic!("invalid shared pool layout: {error}"),
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

/// A fixed shared pool whose complete data region is initialized.
///
/// Unlike [`SharedPool`], this pool may safely expose unused slot capacity as
/// `&mut [u8]`. Initialization happens once when the backing group is
/// allocated; releasing and reacquiring a slot does not clear it.
#[repr(transparent)]
pub struct InitializedSharedPool {
    group: NonNull<Group>,
    marker: PhantomData<*mut ()>,
}

impl InitializedSharedPool {
    /// Creates an initialized pool, returning an error when its fixed
    /// allocation cannot be represented by the pool layout.
    pub fn try_new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        let layout = GroupLayout::new(slots, capacity)?;
        Ok(Self {
            group: Group::allocate(layout, true),
            marker: PhantomData,
        })
    }

    /// Creates an initialized pool with the requested fixed layout.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero or the requested allocation cannot be
    /// represented. Use [`InitializedSharedPool::try_new`] for runtime
    /// configuration.
    #[track_caller]
    pub fn new(slots: usize, capacity: usize) -> Self {
        match Self::try_new(slots, capacity) {
            Ok(pool) => pool,
            Err(error) => panic!("invalid initialized shared pool layout: {error}"),
        }
    }

    pub fn try_acquire(&self) -> Option<InitializedSharedLease> {
        let index = unsafe { Group::acquire(self.group) }?;
        Some(InitializedSharedLease {
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

impl Clone for InitializedSharedPool {
    fn clone(&self) -> Self {
        unsafe { Group::retain(self.group) };
        Self {
            group: self.group,
            marker: PhantomData,
        }
    }
}

impl Drop for InitializedSharedPool {
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

/// A leased slot whose entire capacity contains initialized bytes.
pub struct InitializedSharedLease {
    group: NonNull<Group>,
    index: u32,
    len: u32,
    marker: PhantomData<*mut ()>,
}

impl InitializedSharedLease {
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
        unsafe { slice::from_raw_parts_mut(Group::data(self.group, self.index), self.len()) }
    }

    /// Returns all initialized capacity following the current logical end.
    ///
    /// A newly acquired slot has a logical length of zero, but its bytes retain
    /// the values written during allocation or a previous lease.
    pub fn spare_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        let remaining = self.capacity() - len;
        unsafe {
            slice::from_raw_parts_mut(Group::data(self.group, self.index).add(len), remaining)
        }
    }

    /// Extends the logical length into the initialized spare capacity.
    pub fn try_advance(&mut self, additional: usize) -> Result<(), super::CapacityError> {
        let len = self.len();
        let capacity = self.capacity();
        let attempted = len
            .checked_add(additional)
            .ok_or_else(|| super::CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(super::CapacityError::new(attempted, capacity));
        }
        self.len = attempted as u32;
        Ok(())
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

impl Drop for InitializedSharedLease {
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
