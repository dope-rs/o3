use std::{
    alloc::{GlobalAlloc, Layout, System},
    marker, ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

use o3::collections::completion::narrow::{self, Arena};

struct AllocationCounter;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for AllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: AllocationCounter = AllocationCounter;

pub fn allocation_count() -> usize {
    ALLOCATIONS.load(Ordering::Relaxed)
}

pub struct Owner<'owner, const INDEX_BITS: u32, const GENERATION_BITS: u32> {
    arena: ptr::NonNull<Arena<u64, INDEX_BITS, GENERATION_BITS>>,
    scope: marker::PhantomData<&'owner ()>,
}

unsafe impl<'owner, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    narrow::raw::ArenaOwner<'owner, u64, INDEX_BITS, GENERATION_BITS>
    for Owner<'owner, INDEX_BITS, GENERATION_BITS>
{
    fn arena(self) -> ptr::NonNull<Arena<u64, INDEX_BITS, GENERATION_BITS>> {
        self.arena
    }
}

impl<'owner, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Owner<'owner, INDEX_BITS, GENERATION_BITS>
{
    pub fn new(arena: &mut Arena<u64, INDEX_BITS, GENERATION_BITS>, _scope: &'owner ()) -> Self {
        Self {
            arena: ptr::NonNull::from(arena),
            scope: marker::PhantomData,
        }
    }
}
