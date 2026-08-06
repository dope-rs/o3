use std::{
    alloc::{Layout, alloc, dealloc, handle_alloc_error},
    marker::PhantomData,
    mem::forget,
    num::NonZeroUsize,
    ops::Range,
    ptr::{NonNull, copy, copy_nonoverlapping},
    rc::Rc,
    slice::{from_raw_parts, from_raw_parts_mut},
};

use crate::{ThreadBound, buffer::SpareWriter};

pub(super) mod refs;

use refs::LocalRefCount;

#[repr(C)]
struct Header {
    refs: LocalRefCount,
    capacity: u32,
}

const DATA_OFFSET: usize = size_of::<Header>();
const ALIGN: usize = align_of::<Header>();
const MAX_LAYOUT_SIZE: usize = DATA_OFFSET + u32::MAX as usize;
const VEC_OWNER_TAG: usize = 1;
const _: () = assert!(ALIGN >= 2);
const _: () = assert!(align_of::<Vec<u8>>() >= 2);
const _: () = assert!(MAX_LAYOUT_SIZE <= isize::MAX as usize - (ALIGN - 1));

fn is_span_in_bounds(start: usize, len: usize, capacity: usize) -> bool {
    start.checked_add(len).is_some_and(|end| end <= capacity)
}

fn is_range_in_bounds(src: &Range<usize>, dest: usize, capacity: usize) -> bool {
    src.start <= src.end && src.end <= capacity && is_span_in_bounds(dest, src.len(), capacity)
}

impl Header {
    fn layout(capacity: u32) -> Layout {
        // SAFETY: ALIGN and MAX_LAYOUT_SIZE prove every u32 capacity has a valid rounded layout.
        unsafe { Layout::from_size_align_unchecked(DATA_OFFSET + capacity as usize, ALIGN) }
    }

    fn allocate(capacity: u32) -> NonNull<Header> {
        let layout = Header::layout(capacity);
        let ptr = unsafe { alloc(layout) }.cast::<Header>();
        let Some(ptr) = NonNull::new(ptr) else {
            handle_alloc_error(layout);
        };
        unsafe {
            ptr.write(Header {
                refs: LocalRefCount::one(),
                capacity,
            });
        }
        ptr
    }

    unsafe fn retain(ptr: NonNull<Header>) {
        unsafe { ptr.as_ref() }.refs.retain();
    }

    unsafe fn release(ptr: NonNull<Header>) {
        let header = unsafe { ptr.as_ref() };
        if !header.refs.release() {
            return;
        }
        let layout = Header::layout(header.capacity);
        unsafe { dealloc(ptr.as_ptr().cast(), layout) };
    }

    fn data_ptr(ptr: NonNull<Header>) -> *const u8 {
        unsafe { ptr.as_ptr().cast::<u8>().add(DATA_OFFSET) }
    }

    fn data_mut_ptr(ptr: NonNull<Header>) -> *mut u8 {
        debug_assert!(unsafe { ptr.as_ref() }.refs.is_unique());
        unsafe { ptr.as_ptr().cast::<u8>().add(DATA_OFFSET) }
    }
}

pub(super) struct StorageMut {
    ptr: NonNull<Header>,
    marker: PhantomData<*mut ()>,
}

impl StorageMut {
    pub(super) fn with_capacity_u32(capacity: u32) -> Self {
        Self {
            ptr: Header::allocate(capacity),
            marker: PhantomData,
        }
    }

    pub(super) fn capacity(&self) -> usize {
        unsafe { self.ptr.as_ref() }.capacity as usize
    }

    pub(super) fn initialized(&self, len: usize) -> &[u8] {
        debug_assert!(len <= self.capacity());
        unsafe { from_raw_parts(Header::data_ptr(self.ptr), len) }
    }

    pub(super) fn initialized_mut(&mut self, len: usize) -> &mut [u8] {
        debug_assert!(len <= self.capacity());
        unsafe { from_raw_parts_mut(Header::data_mut_ptr(self.ptr), len) }
    }

    pub(super) fn write_byte(&mut self, offset: usize, byte: u8) {
        debug_assert!(offset < self.capacity());
        unsafe { Header::data_mut_ptr(self.ptr).add(offset).write(byte) };
    }

    pub(super) fn fill(&mut self, byte: u8) {
        unsafe {
            Header::data_mut_ptr(self.ptr).write_bytes(byte, self.capacity());
        };
    }

    pub(super) fn spare_writer<'a>(&'a mut self, target: &'a mut u32) -> SpareWriter<'a> {
        let len = *target as usize;
        let capacity = self.capacity();
        debug_assert!(len <= capacity);
        let ptr = unsafe { Header::data_mut_ptr(self.ptr).add(len).cast() };
        unsafe { SpareWriter::new(ptr, capacity - len, target) }
    }

    pub(super) fn is_unique(&self) -> bool {
        unsafe { self.ptr.as_ref() }.refs.is_unique()
    }

    pub(super) fn share(&self) -> Storage {
        unsafe { Header::retain(self.ptr) };
        Storage {
            ptr: self.ptr,
            marker: PhantomData,
        }
    }

    pub(super) fn freeze(self) -> Storage {
        let storage = Storage {
            ptr: self.ptr,
            marker: PhantomData,
        };
        forget(self);
        storage
    }

    pub(super) fn detach_range(&mut self, src: Range<usize>, dest: usize) -> bool {
        debug_assert!(is_range_in_bounds(&src, dest, self.capacity()));
        if unsafe { self.ptr.as_ref() }.refs.is_unique() {
            return false;
        }
        self.detach_range_slow(src, dest);
        true
    }

    #[cold]
    fn detach_range_slow(&mut self, src: Range<usize>, dest: usize) {
        let ptr = Header::allocate(unsafe { self.ptr.as_ref() }.capacity);
        if !src.is_empty() {
            unsafe {
                copy_nonoverlapping(
                    Header::data_ptr(self.ptr).add(src.start),
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
            copy_nonoverlapping(
                src.as_ptr(),
                Header::data_mut_ptr(self.ptr).add(offset),
                src.len(),
            );
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

    pub(super) fn copy_from_storage(
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
                Header::data_ptr(src.ptr).add(src_offset),
                Header::data_mut_ptr(self.ptr).add(offset),
                len,
            );
        }
    }

    pub(super) fn copy_within(&mut self, src: Range<usize>, dest: usize) {
        debug_assert!(is_range_in_bounds(&src, dest, self.capacity()));
        unsafe {
            let data = Header::data_mut_ptr(self.ptr);
            copy(data.add(src.start), data.add(dest), src.len());
        }
    }
}

impl Drop for StorageMut {
    fn drop(&mut self) {
        unsafe { Header::release(self.ptr) };
    }
}

pub(super) struct Storage {
    ptr: NonNull<Header>,
    marker: PhantomData<*mut ()>,
}

/// A typed, provenance-carrying pointer to a live storage allocation owner.
struct StorageOwner(NonNull<Header>);

impl StorageOwner {
    fn erase(self) -> NonNull<()> {
        self.0.cast()
    }

    /// # Safety
    /// `ptr` must have been returned by [`StorageOwner::erase`] and still denote a
    /// live storage allocation.
    unsafe fn from_erased(ptr: NonNull<()>) -> Self {
        Self(ptr.cast())
    }

    /// # Safety
    /// The allocation must still own at least one live reference.
    unsafe fn retain(self) {
        unsafe { Header::retain(self.0) };
    }

    /// # Safety
    /// This pointer must own one live reference.
    unsafe fn release(self) {
        unsafe { Header::release(self.0) };
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct TaggedOwner(NonNull<()>);

enum OwnerPtr {
    Storage(StorageOwner),
    Vec(NonNull<Vec<u8>>),
}

impl TaggedOwner {
    fn from_storage(storage: Storage) -> Self {
        Self(storage.into_owner().erase())
    }

    fn from_vec(buf: Rc<Vec<u8>>) -> Self {
        let ptr = Rc::into_raw(buf).cast_mut();
        // SAFETY: `Rc::into_raw` never returns a null pointer.
        let ptr = unsafe { NonNull::new_unchecked(ptr) }.cast::<()>();
        Self(ptr.map_addr(|addr| addr | VEC_OWNER_TAG))
    }

    fn decode(self) -> OwnerPtr {
        let tagged = self.0.addr().get() & VEC_OWNER_TAG != 0;
        let ptr = self.0.map_addr(|addr| {
            // SAFETY: both constructors start with a non-null pointer aligned
            // to at least two bytes. Clearing their optional low tag therefore
            // recovers the original non-null address.
            unsafe { NonZeroUsize::new_unchecked(addr.get() & !VEC_OWNER_TAG) }
        });
        if tagged {
            OwnerPtr::Vec(ptr.cast())
        } else {
            // SAFETY: the untagged constructor is exclusively `from_storage`.
            OwnerPtr::Storage(unsafe { StorageOwner::from_erased(ptr) })
        }
    }

    fn retain(self) {
        match self.decode() {
            OwnerPtr::Storage(owner) => unsafe { owner.retain() },
            OwnerPtr::Vec(ptr) => unsafe { Rc::increment_strong_count(ptr.as_ptr()) },
        }
    }

    fn release(self) {
        match self.decode() {
            OwnerPtr::Storage(owner) => unsafe { owner.release() },
            OwnerPtr::Vec(ptr) => unsafe { Rc::decrement_strong_count(ptr.as_ptr()) },
        }
    }
}

const _: () = assert!(size_of::<Option<TaggedOwner>>() == size_of::<NonNull<()>>());

pub(super) struct Owner {
    tagged: Option<TaggedOwner>,
    _thread: ThreadBound,
}

impl Owner {
    pub(super) const NONE: Self = Self {
        tagged: None,
        _thread: ThreadBound::NEW,
    };

    pub(super) fn from_storage(storage: Storage) -> Self {
        Self {
            tagged: Some(TaggedOwner::from_storage(storage)),
            _thread: ThreadBound::NEW,
        }
    }

    pub(super) fn from_vec(buf: Rc<Vec<u8>>) -> Self {
        Self {
            tagged: Some(TaggedOwner::from_vec(buf)),
            _thread: ThreadBound::NEW,
        }
    }
}

impl Clone for Owner {
    fn clone(&self) -> Self {
        if let Some(tagged) = self.tagged {
            tagged.retain();
        }
        Self {
            tagged: self.tagged,
            _thread: ThreadBound::NEW,
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(tagged) = self.tagged {
            tagged.release();
        }
    }
}

pub(super) struct StorageSpan {
    storage: Storage,
    ptr: *const u8,
    len: usize,
}

impl StorageSpan {
    /// # Safety
    /// `start..start + len` must be in bounds of `storage`.
    pub(super) unsafe fn new_unchecked(storage: Storage, start: u32, len: u32) -> Self {
        debug_assert!(
            start
                .checked_add(len)
                .is_some_and(|end| end as usize <= storage.capacity())
        );
        Self {
            ptr: unsafe { Header::data_ptr(storage.ptr).add(start as usize) },
            len: len as usize,
            storage,
        }
    }

    pub(super) fn copy_from_slice(slice: &[u8]) -> Option<Self> {
        let Ok(len) = u32::try_from(slice.len()) else {
            return None;
        };
        let mut storage = StorageMut::with_capacity_u32(len);
        storage.copy_from_slice(0, slice);
        // SAFETY: the allocation capacity is exactly `len`.
        Some(unsafe { Self::new_unchecked(storage.freeze(), 0, len) })
    }

    pub(super) fn into_parts(self) -> (Storage, *const u8, usize) {
        (self.storage, self.ptr, self.len)
    }
}

impl Storage {
    pub(super) fn capacity(&self) -> usize {
        unsafe { self.ptr.as_ref() }.capacity as usize
    }

    fn into_owner(self) -> StorageOwner {
        let owner = StorageOwner(self.ptr);
        forget(self);
        owner
    }
}

impl Clone for Storage {
    fn clone(&self) -> Self {
        unsafe { Header::retain(self.ptr) };
        Self {
            ptr: self.ptr,
            marker: PhantomData,
        }
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        unsafe { Header::release(self.ptr) };
    }
}
