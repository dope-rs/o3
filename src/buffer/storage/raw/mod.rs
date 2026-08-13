use std::{alloc, marker, mem, ops, ptr, slice};

use crate::{
    buffer::{self, resident, write},
    cell,
};

mod owner;

pub(in crate::buffer) use owner::Owner;

#[repr(C)]
pub(super) struct Prefix {
    pub(super) refs: cell::LocalRefCount,
    pub(super) capacity: u32,
}

#[repr(C)]
pub(super) struct Header<P> {
    prefix: Prefix,
    policy: P,
}

const _: () = assert!(size_of::<Header<()>>() == size_of::<Prefix>());
const _: () = assert!(align_of::<Header<()>>() >= 4);
const _: () = assert!(align_of::<Header<resident::Lease>>() >= 4);
const _: () = assert!(align_of::<Vec<u8>>() >= 4);

fn is_span_in_bounds(start: usize, len: usize, capacity: usize) -> bool {
    start.checked_add(len).is_some_and(|end| end <= capacity)
}

fn is_range_in_bounds(src: &ops::Range<usize>, dest: usize, capacity: usize) -> bool {
    src.start <= src.end && src.end <= capacity && is_span_in_bounds(dest, src.len(), capacity)
}

impl<P> Header<P> {
    const DATA_OFFSET: usize = size_of::<Self>();
    const ALIGN: usize = align_of::<Self>();
    const MAX_LAYOUT_SIZE: usize = Self::DATA_OFFSET + u32::MAX as usize;
    const VALID: () = assert!(Self::MAX_LAYOUT_SIZE <= isize::MAX as usize - (Self::ALIGN - 1));

    fn layout(capacity: u32) -> alloc::Layout {
        let () = Self::VALID;
        // SAFETY: ALIGN and MAX_LAYOUT_SIZE prove every u32 capacity has a valid rounded layout.
        unsafe {
            alloc::Layout::from_size_align_unchecked(
                Self::DATA_OFFSET + capacity as usize,
                Self::ALIGN,
            )
        }
    }

    fn allocate(capacity: u32, policy: P) -> ptr::NonNull<Self> {
        let layout = Self::layout(capacity);
        let ptr = unsafe {
            use std::alloc::alloc;
            alloc(layout)
        }
        .cast::<Self>();
        let Some(ptr) = ptr::NonNull::new(ptr) else {
            use std::alloc::handle_alloc_error;
            handle_alloc_error(layout);
        };
        unsafe {
            ptr.write(Self {
                prefix: Prefix {
                    refs: cell::LocalRefCount::one(),
                    capacity,
                },
                policy,
            });
        }
        ptr
    }

    unsafe fn retain(ptr: ptr::NonNull<Self>) {
        unsafe { ptr.as_ref() }.prefix.refs.retain();
    }

    unsafe fn release(ptr: ptr::NonNull<Self>) {
        let header = unsafe { ptr.as_ref() };
        if !header.prefix.refs.release() {
            return;
        }
        unsafe { Self::destroy(ptr) };
    }

    pub(super) unsafe fn destroy(ptr: ptr::NonNull<Self>) {
        let layout = Self::layout(unsafe { ptr.as_ref() }.prefix.capacity);
        unsafe {
            use std::alloc::dealloc;
            ptr::drop_in_place(ptr.as_ptr());
            dealloc(ptr.as_ptr().cast(), layout);
        }
    }

    fn data_ptr(ptr: ptr::NonNull<Self>) -> *const u8 {
        unsafe { ptr.as_ptr().cast::<u8>().add(Self::DATA_OFFSET) }
    }

    fn data_mut_ptr(ptr: ptr::NonNull<Self>) -> *mut u8 {
        debug_assert!(unsafe { ptr.as_ref() }.prefix.refs.is_unique());
        unsafe { ptr.as_ptr().cast::<u8>().add(Self::DATA_OFFSET) }
    }
}

pub(in crate::buffer) struct AllocationMut<P = ()> {
    ptr: ptr::NonNull<Header<P>>,
    marker: marker::PhantomData<*mut ()>,
}

pub(in crate::buffer) struct BytesMut<'a, P> {
    allocation: &'a mut AllocationMut<P>,
}

const _: () = assert!(size_of::<AllocationMut<()>>() == size_of::<ptr::NonNull<()>>());

impl AllocationMut<()> {
    pub(in crate::buffer) fn with_capacity_u32(capacity: u32) -> Self {
        Self {
            ptr: Header::allocate(capacity, ()),
            marker: marker::PhantomData,
        }
    }

    pub(in crate::buffer) fn grow_unique(&mut self, capacity: u32) {
        self.realloc_unique(capacity);
    }
}

impl AllocationMut<resident::Lease> {
    pub(in crate::buffer) fn with_budget_zero(budget: &resident::Budget<'_>) -> Self {
        Self {
            ptr: Header::allocate(0, budget.acquire_zero()),
            marker: marker::PhantomData,
        }
    }

    pub(in crate::buffer) fn with_budget(
        capacity: u32,
        budget: &resident::Budget<'_>,
    ) -> Result<Self, buffer::CapacityError> {
        Ok(Self {
            ptr: Header::allocate(capacity, budget.acquire(capacity)?),
            marker: marker::PhantomData,
        })
    }

    pub(in crate::buffer) fn sibling(&self, capacity: u32) -> Result<Self, buffer::CapacityError> {
        let policy = unsafe { self.ptr.as_ref() }.policy.sibling(capacity)?;
        Ok(Self {
            ptr: Header::allocate(capacity, policy),
            marker: marker::PhantomData,
        })
    }

    pub(in crate::buffer) fn grow_unique(
        &mut self,
        capacity: u32,
    ) -> Result<(), buffer::CapacityError> {
        debug_assert!(self.is_unique());
        unsafe { self.ptr.as_mut() }.policy.grow(capacity)?;
        self.realloc_unique(capacity);
        Ok(())
    }
}

impl<P> AllocationMut<P> {
    fn realloc_unique(&mut self, capacity: u32) {
        debug_assert!(self.is_unique());
        let old_layout = Header::<P>::layout(unsafe { self.ptr.as_ref() }.prefix.capacity);
        let new_layout = Header::<P>::layout(capacity);
        let ptr = unsafe {
            use std::alloc::realloc;
            realloc(self.ptr.as_ptr().cast(), old_layout, new_layout.size())
        }
        .cast::<Header<P>>();
        let Some(mut ptr) = ptr::NonNull::new(ptr) else {
            use std::alloc::handle_alloc_error;
            handle_alloc_error(new_layout);
        };
        unsafe { ptr.as_mut() }.prefix.capacity = capacity;
        self.ptr = ptr;
    }

    pub(in crate::buffer) fn capacity(&self) -> usize {
        unsafe { self.ptr.as_ref() }.prefix.capacity as usize
    }

    pub(in crate::buffer) fn initialized(&self, len: usize) -> &[u8] {
        debug_assert!(len <= self.capacity());
        unsafe {
            use std::slice::from_raw_parts;
            from_raw_parts(Header::data_ptr(self.ptr), len)
        }
    }

    pub(in crate::buffer) fn bytes_mut(&mut self) -> BytesMut<'_, P> {
        BytesMut { allocation: self }
    }

    pub(in crate::buffer) fn is_unique(&self) -> bool {
        unsafe { self.ptr.as_ref() }.prefix.refs.is_unique()
    }

    pub(in crate::buffer) fn share(&self) -> Allocation<P> {
        unsafe { Header::retain(self.ptr) };
        Allocation {
            ptr: self.ptr,
            marker: marker::PhantomData,
        }
    }

    pub(in crate::buffer) fn freeze(self) -> Allocation<P> {
        let allocation = Allocation {
            ptr: self.ptr,
            marker: marker::PhantomData,
        };
        mem::forget(self);
        allocation
    }
}

impl<'a, P> BytesMut<'a, P> {
    pub(in crate::buffer) fn initialized(self, len: usize) -> &'a mut [u8] {
        debug_assert!(len <= self.allocation.capacity());
        unsafe {
            use std::slice::from_raw_parts_mut;
            from_raw_parts_mut(Header::data_mut_ptr(self.allocation.ptr), len)
        }
    }

    pub(in crate::buffer) fn write_byte(&mut self, offset: usize, byte: u8) {
        debug_assert!(offset < self.allocation.capacity());
        unsafe {
            Header::data_mut_ptr(self.allocation.ptr)
                .add(offset)
                .write(byte)
        };
    }

    pub(in crate::buffer) fn fill(&mut self, byte: u8) {
        unsafe {
            Header::data_mut_ptr(self.allocation.ptr).write_bytes(byte, self.allocation.capacity());
        };
    }

    pub(in crate::buffer) fn spare_writer(self, target: &'a mut u32) -> write::SpareWriter<'a> {
        let len = *target as usize;
        let capacity = self.allocation.capacity();
        debug_assert!(len <= capacity);
        // SAFETY: `len..capacity` lies in this uniquely borrowed allocation and
        // is precisely the uninitialized suffix represented by the writer.
        let spare = unsafe {
            slice::from_raw_parts_mut(
                Header::data_mut_ptr(self.allocation.ptr).add(len).cast(),
                capacity - len,
            )
        };
        write::SpareWriter::new(spare, target)
    }

    pub(in crate::buffer) fn copy_from_slice(&mut self, offset: usize, src: &[u8]) {
        debug_assert!(is_span_in_bounds(
            offset,
            src.len(),
            self.allocation.capacity()
        ));
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                Header::data_mut_ptr(self.allocation.ptr).add(offset),
                src.len(),
            );
        }
    }

    /// # Safety
    /// The destination is in bounds and overlaps neither `src` nor any shared range.
    pub(in crate::buffer) unsafe fn copy_from_slice_disjoint(&mut self, offset: usize, src: &[u8]) {
        debug_assert!(is_span_in_bounds(
            offset,
            src.len(),
            self.allocation.capacity()
        ));
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                self.allocation
                    .ptr
                    .as_ptr()
                    .cast::<u8>()
                    .add(Header::<P>::DATA_OFFSET + offset),
                src.len(),
            );
        }
    }

    pub(in crate::buffer) fn copy_from_allocation(
        &mut self,
        offset: usize,
        src: &AllocationMut<P>,
        src_offset: usize,
        len: usize,
    ) {
        debug_assert!(
            is_span_in_bounds(offset, len, self.allocation.capacity())
                && is_span_in_bounds(src_offset, len, src.capacity())
        );
        unsafe {
            ptr::copy_nonoverlapping(
                Header::data_ptr(src.ptr).add(src_offset),
                Header::data_mut_ptr(self.allocation.ptr).add(offset),
                len,
            );
        }
    }

    pub(in crate::buffer) fn copy_within(&mut self, src: ops::Range<usize>, dest: usize) {
        debug_assert!(is_range_in_bounds(&src, dest, self.allocation.capacity()));
        unsafe {
            use std::ptr::copy;
            let data = Header::data_mut_ptr(self.allocation.ptr);
            copy(data.add(src.start), data.add(dest), src.len());
        }
    }
}

impl<P> Drop for AllocationMut<P> {
    fn drop(&mut self) {
        unsafe { Header::release(self.ptr) };
    }
}

pub(in crate::buffer) struct Allocation<P = ()> {
    ptr: ptr::NonNull<Header<P>>,
    marker: marker::PhantomData<*mut ()>,
}

/// A typed, provenance-carrying pointer to a live storage allocation owner.
pub(super) struct AllocationOwner<P>(ptr::NonNull<Header<P>>);

impl<P> AllocationOwner<P> {
    pub(super) fn erase(self) -> ptr::NonNull<()> {
        self.0.cast()
    }
}

pub(in crate::buffer) struct Span<P = ()> {
    allocation: Allocation<P>,
    ptr: *const u8,
    len: usize,
}

impl<P> Span<P> {
    /// # Safety
    /// `start..start + len` must be in bounds of `allocation`.
    pub(in crate::buffer) unsafe fn new_unchecked(
        allocation: Allocation<P>,
        start: u32,
        len: u32,
    ) -> Self {
        debug_assert!(
            start
                .checked_add(len)
                .is_some_and(|end| end as usize <= allocation.capacity())
        );
        Self {
            ptr: unsafe { Header::data_ptr(allocation.ptr).add(start as usize) },
            len: len as usize,
            allocation,
        }
    }

    pub(in crate::buffer) fn into_parts(self) -> (Allocation<P>, *const u8, usize) {
        (self.allocation, self.ptr, self.len)
    }
}

impl Span<()> {
    pub(in crate::buffer) fn copy_from_slice(slice: &[u8]) -> Option<Self> {
        let Ok(len) = u32::try_from(slice.len()) else {
            return None;
        };
        Some(Self::copy_from_bounded_slice(slice, len))
    }

    pub(in crate::buffer) fn copy_from_bounded_slice(slice: &[u8], len: u32) -> Self {
        debug_assert_eq!(slice.len(), len as usize);
        let mut allocation = AllocationMut::with_capacity_u32(len);
        allocation.bytes_mut().copy_from_slice(0, slice);
        // SAFETY: the allocation capacity is exactly `len`.
        unsafe { Self::new_unchecked(allocation.freeze(), 0, len) }
    }
}

impl<P> Allocation<P> {
    pub(in crate::buffer) fn capacity(&self) -> usize {
        unsafe { self.ptr.as_ref() }.prefix.capacity as usize
    }

    pub(super) fn into_owner(self) -> AllocationOwner<P> {
        let owner = AllocationOwner(self.ptr);
        mem::forget(self);
        owner
    }
}

impl<P> Clone for Allocation<P> {
    fn clone(&self) -> Self {
        unsafe { Header::retain(self.ptr) };
        Self {
            ptr: self.ptr,
            marker: marker::PhantomData,
        }
    }
}

impl<P> Drop for Allocation<P> {
    fn drop(&mut self) {
        unsafe { Header::release(self.ptr) };
    }
}
