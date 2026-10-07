use std::{mem, ops, ptr};

use crate::alloc::{self, tail};

/// Ownership of one allocated header-and-tail layout before initialization.
#[must_use = "initialize the allocation or release it"]
pub struct Uninit<H, T, N: alloc::raw::TailExtent = usize> {
    pub(in crate::alloc::tail) memory: ptr::NonNull<u8>,
    pub(in crate::alloc::tail) layout: tail::Plan<H, T, N>,
}

impl<H, T, N: alloc::raw::TailExtent> Uninit<H, T, N> {
    pub fn fill(self, header: H, mut init: impl FnMut(&H, N::Index) -> T) -> tail::Block<H, T, N> {
        let mut block = self.init(header);
        let indices = block.extent().indices();
        let (header, tail) = block.parts_mut();
        let header: &H = header;
        for (index, slot) in indices.zip(tail) {
            slot.write(init(header, index));
        }
        // SAFETY: the exact extent visited and initialized every tail slot.
        unsafe { block.assume_init() }
    }

    pub fn repeat(self, header: H, value: T) -> tail::Block<H, T, N>
    where
        T: Copy,
    {
        self.repeat_with(header, || value)
    }

    pub fn repeat_with(self, header: H, mut init: impl FnMut() -> T) -> tail::Block<H, T, N> {
        let mut block = self.init(header);
        alloc::raw::init(block.as_mut_slice(), |_| init());
        // SAFETY: every tail slot now contains one initialized element.
        unsafe { block.assume_init() }
    }

    /// Initializes the header while retaining the tail as uninitialized storage.
    pub fn init(self, header: H) -> tail::Block<H, mem::MaybeUninit<T>, N> {
        let this = mem::ManuallyDrop::new(self);
        let pointer = this.memory.cast::<tail::Prefix<H, T, N>>();
        // SAFETY: the allocation uses the exact combined layout, is uniquely
        // owned, and its prefix is currently uninitialized.
        unsafe {
            pointer.as_ptr().write(tail::Prefix {
                header,
                len: this.layout,
            })
        };
        tail::Block {
            pointer: pointer.cast(),
        }
    }
}

impl<H, T, N: alloc::raw::TailExtent> Drop for Uninit<H, T, N> {
    fn drop(&mut self) {
        // SAFETY: this state owns the exact allocation and has initialized no value.
        unsafe { alloc::raw::LayoutExt::deallocate(self.layout.layout(), self.memory) };
    }
}

impl<H, T> Uninit<H, T> {
    /// Initializes the immovable header and lends each tail write in order.
    pub fn start(self, header: H) -> ops::ControlFlow<tail::Block<H, T>, tail::Entry<H, T>> {
        tail::Entry::new(self.init(header))
    }
}
