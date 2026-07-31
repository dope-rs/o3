use std::{
    alloc::{Layout, alloc, alloc_zeroed, dealloc, handle_alloc_error},
    cell::Cell,
    marker::PhantomData,
    mem::ManuallyDrop,
    num::NonZeroU32,
    ptr::NonNull,
    slice,
};

use super::{
    super::{CapacityError, LocalRefCount, PrefixLength, SpareWriter},
    PoolLayoutError,
};

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

#[derive(Clone, Copy, Debug)]
pub struct SharedPoolLayout {
    allocation: Layout,
    slots: u32,
    capacity: NonZeroU32,
    data_offset: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct SharedPoolPlan {
    max_slots: usize,
    capacity: usize,
}

impl SharedPoolLayout {
    pub fn new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
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

    #[must_use]
    pub fn fixed<const SLOTS: usize, const CAPACITY: usize>() -> Self {
        const {
            assert!(SLOTS <= u32::MAX as usize);
            assert!(CAPACITY != 0);
            assert!(CAPACITY <= u32::MAX as usize);
            let slot_bytes = SLOTS as u128 * size_of::<Slot>() as u128;
            let data_bytes = SLOTS as u128 * CAPACITY as u128;
            let padding = align_of::<Group>() as u128 + align_of::<Slot>() as u128;
            let total = size_of::<Group>() as u128 + slot_bytes + data_bytes + padding;
            assert!(total <= isize::MAX as u128);
        }
        // SAFETY: the const proof covers every conversion and Layout size bound.
        unsafe { Self::new(SLOTS, CAPACITY).unwrap_unchecked() }
    }

    pub const fn slots(self) -> usize {
        self.slots as usize
    }

    pub const fn capacity(self) -> usize {
        self.capacity.get() as usize
    }
}

impl SharedPoolPlan {
    pub fn new(max_slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        SharedPoolLayout::new(max_slots, capacity)?;
        Ok(Self {
            max_slots,
            capacity,
        })
    }

    #[must_use]
    pub fn fixed<const MAX_SLOTS: usize, const CAPACITY: usize>() -> Self {
        let _ = SharedPoolLayout::fixed::<MAX_SLOTS, CAPACITY>();
        Self {
            max_slots: MAX_SLOTS,
            capacity: CAPACITY,
        }
    }

    pub fn layout_up_to(self, requested: usize) -> SharedPoolLayout {
        let slots = requested.min(self.max_slots);
        // SAFETY: the maximum layout was validated and reducing slots cannot overflow it.
        unsafe { SharedPoolLayout::new(slots, self.capacity).unwrap_unchecked() }
    }

    pub fn layout(self) -> SharedPoolLayout {
        self.layout_up_to(self.max_slots)
    }

    pub const fn max_slots(self) -> usize {
        self.max_slots
    }

    pub const fn capacity(self) -> usize {
        self.capacity
    }
}

#[doc(hidden)]
pub trait SharedPoolState: private::Sealed {
    unsafe fn allocate(layout: Layout) -> *mut u8;
}

mod private {
    pub trait Sealed {}
}

#[doc(hidden)]
pub struct Uninitialized;

impl private::Sealed for Uninitialized {}

impl SharedPoolState for Uninitialized {
    unsafe fn allocate(layout: Layout) -> *mut u8 {
        unsafe { alloc(layout) }
    }
}

#[doc(hidden)]
pub struct Initialized;

impl private::Sealed for Initialized {}

impl SharedPoolState for Initialized {
    unsafe fn allocate(layout: Layout) -> *mut u8 {
        unsafe { alloc_zeroed(layout) }
    }
}

impl Group {
    fn allocate<S: SharedPoolState>(layout: SharedPoolLayout) -> NonNull<Self> {
        let raw = unsafe { S::allocate(layout.allocation) };
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

#[repr(transparent)]
pub struct SharedPool<S: SharedPoolState = Uninitialized> {
    group: NonNull<Group>,
    marker: PhantomData<(S, *mut ())>,
}

impl<S: SharedPoolState> SharedPool<S> {
    pub fn from_layout(layout: SharedPoolLayout) -> Self {
        Self {
            group: Group::allocate::<S>(layout),
            marker: PhantomData,
        }
    }

    /// Creates a pool, returning an error when its fixed allocation cannot be
    /// represented by the pool layout.
    pub fn try_new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        Ok(Self::from_layout(SharedPoolLayout::new(slots, capacity)?))
    }

    pub fn try_acquire(&self) -> Option<SharedLease<S>> {
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

impl<S: SharedPoolState> Clone for SharedPool<S> {
    fn clone(&self) -> Self {
        unsafe { Group::retain(self.group) };
        Self {
            group: self.group,
            marker: PhantomData,
        }
    }
}

impl<S: SharedPoolState> Drop for SharedPool<S> {
    fn drop(&mut self) {
        unsafe { Group::release(self.group) };
    }
}

pub struct SharedLease<S: SharedPoolState = Uninitialized> {
    group: NonNull<Group>,
    index: u32,
    len: u32,
    marker: PhantomData<(S, *mut ())>,
}

impl<S: SharedPoolState> SharedLease<S> {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(Group::data(self.group, self.index), self.len as usize) }
    }

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

    pub fn freeze(self) -> Pooled {
        let this = ManuallyDrop::new(self);
        Pooled {
            group: this.group,
            index: this.index,
            len: this.len,
            marker: PhantomData,
        }
    }
}

impl SharedLease<Uninitialized> {
    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        let group = unsafe { self.group.as_ref() };
        let len = self.len as usize;
        let ptr = unsafe { Group::data(self.group, self.index).add(len).cast() };
        unsafe { SpareWriter::new(ptr, group.capacity as usize - len, &mut self.len) }
    }
}

impl<S: SharedPoolState> Drop for SharedLease<S> {
    fn drop(&mut self) {
        unsafe { Group::release_slot(self.group, self.index) };
    }
}

impl<S: SharedPoolState> PrefixLength for SharedLease<S> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl SharedLease<Initialized> {
    /// Returns initialized capacity after the logical end.
    ///
    /// Reacquired slots retain values written by their previous lease.
    pub fn spare_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        let remaining = self.capacity() - len;
        unsafe {
            slice::from_raw_parts_mut(Group::data(self.group, self.index).add(len), remaining)
        }
    }

    /// Extends the logical length into the initialized spare capacity.
    pub fn try_advance(&mut self, additional: usize) -> Result<(), CapacityError> {
        let len = self.len();
        let capacity = self.capacity();
        let attempted = len
            .checked_add(additional)
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(CapacityError::new(attempted, capacity));
        }
        self.len = attempted as u32;
        Ok(())
    }
}

pub struct Pooled {
    group: NonNull<Group>,
    index: u32,
    len: u32,
    marker: PhantomData<*mut ()>,
}

impl Pooled {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(Group::data(self.group, self.index), self.len as usize) }
    }
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
