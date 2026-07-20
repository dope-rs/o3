use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::cell::Cell;
use std::marker::PhantomData;
use std::mem::forget;
use std::ops::Range;
use std::ptr::{NonNull, copy, copy_nonoverlapping};

#[repr(C)]
struct Header {
    refs: Cell<u32>,
    capacity: u32,
}

const DATA_OFFSET: usize = size_of::<Header>();
const ALIGN: usize = align_of::<Header>();

fn is_span_in_bounds(start: usize, len: usize, capacity: usize) -> bool {
    start.checked_add(len).is_some_and(|end| end <= capacity)
}

fn is_range_in_bounds(src: &Range<usize>, dest: usize, capacity: usize) -> bool {
    src.start <= src.end && src.end <= capacity && is_span_in_bounds(dest, src.len(), capacity)
}

impl Header {
    fn layout(capacity: u32) -> Layout {
        let size = DATA_OFFSET
            .checked_add(capacity as usize)
            .expect("buffer capacity overflow");
        Layout::from_size_align(size, ALIGN).expect("buffer layout invariant")
    }

    fn allocate(capacity: usize) -> NonNull<Header> {
        assert!(u32::try_from(capacity).is_ok(), "buffer capacity overflow");
        let capacity = capacity as u32;
        let layout = Header::layout(capacity);
        let ptr = unsafe { alloc(layout) }.cast::<Header>();
        let Some(ptr) = NonNull::new(ptr) else {
            handle_alloc_error(layout);
        };
        unsafe {
            ptr.write(Header {
                refs: Cell::new(1),
                capacity,
            });
        }
        ptr
    }

    unsafe fn retain(ptr: NonNull<Header>) {
        let refs = unsafe { ptr.as_ref() }.refs.get();
        assert!(refs != u32::MAX, "buffer reference overflow");
        unsafe { ptr.as_ref() }.refs.set(refs + 1);
    }

    unsafe fn release(ptr: NonNull<Header>) {
        let header = unsafe { ptr.as_ref() };
        let refs = header.refs.get();
        debug_assert_ne!(refs, 0);
        if refs != 1 {
            header.refs.set(refs - 1);
            return;
        }
        let layout = Header::layout(header.capacity);
        unsafe { dealloc(ptr.as_ptr().cast(), layout) };
    }
}

pub(super) struct RawMut {
    ptr: NonNull<Header>,
    marker: PhantomData<*mut ()>,
}

impl RawMut {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            ptr: Header::allocate(capacity),
            marker: PhantomData,
        }
    }

    pub(super) fn into_data(mut self) -> NonNull<u8> {
        let ptr = unsafe { NonNull::new_unchecked(self.data_mut_ptr()) };
        forget(self);
        ptr
    }

    /// # Safety
    /// `ptr` came from `RawMut::into_data` and still owns that allocation.
    pub(super) unsafe fn from_data(ptr: NonNull<u8>) -> Self {
        Self {
            ptr: unsafe { NonNull::new_unchecked(ptr.as_ptr().sub(DATA_OFFSET).cast()) },
            marker: PhantomData,
        }
    }

    pub(super) fn capacity(&self) -> usize {
        unsafe { self.ptr.as_ref() }.capacity as usize
    }

    pub(super) fn data_ptr(&self) -> *const u8 {
        unsafe { self.ptr.as_ptr().cast::<u8>().add(DATA_OFFSET) }
    }

    pub(super) fn data_mut_ptr(&mut self) -> *mut u8 {
        debug_assert_eq!(unsafe { self.ptr.as_ref() }.refs.get(), 1);
        unsafe { self.ptr.as_ptr().cast::<u8>().add(DATA_OFFSET) }
    }

    pub(super) fn is_unique(&self) -> bool {
        unsafe { self.ptr.as_ref() }.refs.get() == 1
    }

    pub(super) fn share(&self) -> Raw {
        unsafe { Header::retain(self.ptr) };
        Raw {
            ptr: self.ptr,
            marker: PhantomData,
        }
    }

    pub(super) fn freeze(self) -> Raw {
        let raw = Raw {
            ptr: self.ptr,
            marker: PhantomData,
        };
        forget(self);
        raw
    }

    pub(super) fn detach_range(&mut self, src: Range<usize>, dest: usize) -> bool {
        debug_assert!(is_range_in_bounds(&src, dest, self.capacity()));
        if unsafe { self.ptr.as_ref() }.refs.get() == 1 {
            return false;
        }
        self.detach_range_slow(src, dest);
        true
    }

    #[cold]
    fn detach_range_slow(&mut self, src: Range<usize>, dest: usize) {
        let ptr = Header::allocate(self.capacity());
        if !src.is_empty() {
            unsafe {
                copy_nonoverlapping(
                    self.data_ptr().add(src.start),
                    ptr.as_ptr().cast::<u8>().add(DATA_OFFSET + dest),
                    src.len(),
                );
            }
        }
        unsafe { Header::release(self.ptr) };
        self.ptr = ptr;
    }

    pub(super) fn copy_from_slice(&mut self, offset: usize, src: &[u8]) {
        debug_assert!(is_span_in_bounds(offset, src.len(), self.capacity()));
        unsafe {
            copy_nonoverlapping(src.as_ptr(), self.data_mut_ptr().add(offset), src.len());
        }
    }

    /// # Safety
    /// The destination is in bounds and overlaps neither `src` nor any shared range.
    pub(super) unsafe fn copy_from_slice_disjoint(&mut self, offset: usize, src: &[u8]) {
        debug_assert!(is_span_in_bounds(offset, src.len(), self.capacity()));
        unsafe {
            copy_nonoverlapping(
                src.as_ptr(),
                self.ptr.as_ptr().cast::<u8>().add(DATA_OFFSET + offset),
                src.len(),
            );
        }
    }

    pub(super) fn copy_from_raw(
        &mut self,
        offset: usize,
        src: &Self,
        src_offset: usize,
        len: usize,
    ) {
        debug_assert!(
            is_span_in_bounds(offset, len, self.capacity())
                && is_span_in_bounds(src_offset, len, src.capacity())
        );
        unsafe {
            copy_nonoverlapping(
                src.data_ptr().add(src_offset),
                self.data_mut_ptr().add(offset),
                len,
            );
        }
    }

    pub(super) fn copy_within(&mut self, src: Range<usize>, dest: usize) {
        debug_assert!(is_range_in_bounds(&src, dest, self.capacity()));
        unsafe {
            let data = self.data_mut_ptr();
            copy(data.add(src.start), data.add(dest), src.len());
        }
    }
}

impl Drop for RawMut {
    fn drop(&mut self) {
        unsafe { Header::release(self.ptr) };
    }
}

pub(super) struct Raw {
    ptr: NonNull<Header>,
    marker: PhantomData<*mut ()>,
}

impl Raw {
    pub(super) fn from_slice(slice: &[u8]) -> Self {
        let mut data = RawMut::with_capacity(slice.len());
        data.copy_from_slice(0, slice);
        data.freeze()
    }

    pub(super) fn data_ptr(&self) -> *const u8 {
        unsafe { self.ptr.as_ptr().cast::<u8>().add(DATA_OFFSET) }
    }

    pub(super) fn capacity(&self) -> usize {
        unsafe { self.ptr.as_ref() }.capacity as usize
    }
}

impl Clone for Raw {
    fn clone(&self) -> Self {
        unsafe { Header::retain(self.ptr) };
        Self {
            ptr: self.ptr,
            marker: PhantomData,
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { Header::release(self.ptr) };
    }
}
