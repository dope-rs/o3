use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::cell::Cell;
use std::marker::PhantomData;
use std::ptr::NonNull;

#[repr(C)]
struct RawHeader {
    refcount: Cell<u32>,
    capacity: u32,
}

const HEADER_SIZE: usize = std::mem::size_of::<RawHeader>();
const HEADER_ALIGN: usize = std::mem::align_of::<RawHeader>();

fn layout_for(payload: usize) -> Layout {
    let total = HEADER_SIZE
        .checked_add(payload)
        .expect("buffer::Raw: layout overflow");
    Layout::from_size_align(total, HEADER_ALIGN).expect("buffer::Raw: layout invariant")
}

unsafe fn alloc_header(cap: usize) -> NonNull<RawHeader> {
    assert!(
        cap <= u32::MAX as usize,
        "buffer::Raw: capacity too large ({cap}, max {})",
        u32::MAX
    );
    let layout = layout_for(cap);
    let raw = unsafe { alloc(layout) };
    if raw.is_null() {
        handle_alloc_error(layout);
    }
    let header_ptr = raw as *mut RawHeader;
    unsafe {
        header_ptr.write(RawHeader {
            refcount: Cell::new(1),
            capacity: cap as u32,
        });
        NonNull::new_unchecked(header_ptr)
    }
}

unsafe fn dealloc_buffer(ptr: NonNull<RawHeader>) {
    unsafe {
        let cap = ptr.as_ref().capacity as usize;
        let layout = layout_for(cap);
        std::ptr::drop_in_place(ptr.as_ptr());
        dealloc(ptr.as_ptr() as *mut u8, layout);
    }
}

unsafe fn refcount_inc(ptr: NonNull<RawHeader>) {
    unsafe {
        let header = ptr.as_ref();
        let next = header
            .refcount
            .get()
            .checked_add(1)
            .expect("buffer::Raw: refcount overflow");
        header.refcount.set(next);
    }
}

unsafe fn refcount_dec(ptr: NonNull<RawHeader>) {
    unsafe {
        let header = ptr.as_ref();
        let prev = header.refcount.get();
        debug_assert!(prev > 0, "buffer::Raw: drop with zero refcount");
        let next = prev - 1;
        if next != 0 {
            header.refcount.set(next);
            return;
        }
        dealloc_buffer(ptr);
    }
}

pub struct RawMut {
    ptr: NonNull<RawHeader>,
    _marker: PhantomData<*mut ()>,
}

impl RawMut {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            ptr: unsafe { alloc_header(cap) },
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn capacity(&self) -> u32 {
        unsafe { self.ptr.as_ref().capacity }
    }

    #[inline]
    pub fn data_ptr(&self) -> *const u8 {
        unsafe { (self.ptr.as_ptr() as *const u8).add(HEADER_SIZE) }
    }

    #[inline]
    pub fn data_mut_ptr(&mut self) -> *mut u8 {
        unsafe { (self.ptr.as_ptr() as *mut u8).add(HEADER_SIZE) }
    }

    pub fn share(&self) -> Raw {
        unsafe { refcount_inc(self.ptr) };
        Raw {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }

    pub fn freeze(self) -> Raw {
        let ptr = self.ptr;
        std::mem::forget(self);
        Raw {
            ptr,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn refcount(&self) -> u32 {
        unsafe { self.ptr.as_ref().refcount.get() }
    }

    #[inline]
    pub fn ensure_unique_for_mutate(&mut self, keep: usize) {
        if self.refcount() == 1 {
            return;
        }
        self.cow_swap(keep);
    }

    #[cold]
    fn cow_swap(&mut self, keep: usize) {
        let cap = self.capacity() as usize;
        debug_assert!(keep <= cap, "buffer::RawMut::cow_swap: keep > cap");
        let new_ptr = unsafe { alloc_header(cap) };
        if keep > 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (self.ptr.as_ptr() as *const u8).add(HEADER_SIZE),
                    (new_ptr.as_ptr() as *mut u8).add(HEADER_SIZE),
                    keep,
                );
            }
        }
        unsafe { refcount_dec(self.ptr) };
        self.ptr = new_ptr;
    }
}

impl Drop for RawMut {
    fn drop(&mut self) {
        unsafe { refcount_dec(self.ptr) };
    }
}

pub struct Raw {
    ptr: NonNull<RawHeader>,
    _marker: PhantomData<*mut ()>,
}

impl Raw {
    pub fn from_slice(slice: &[u8]) -> Self {
        let mut buf = RawMut::with_capacity(slice.len());
        if !slice.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(slice.as_ptr(), buf.data_mut_ptr(), slice.len());
            }
        }
        buf.freeze()
    }

    #[inline]
    pub fn data_ptr(&self) -> *const u8 {
        unsafe { (self.ptr.as_ptr() as *const u8).add(HEADER_SIZE) }
    }

    #[inline]
    pub fn capacity(&self) -> u32 {
        unsafe { self.ptr.as_ref().capacity }
    }

    #[inline]
    pub fn refcount(&self) -> u32 {
        unsafe { self.ptr.as_ref().refcount.get() }
    }
}

impl Clone for Raw {
    #[inline]
    fn clone(&self) -> Self {
        unsafe { refcount_inc(self.ptr) };
        Self {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { refcount_dec(self.ptr) };
    }
}
