use std::{mem, ops, ptr};

use crate::alloc::tail;

/// Sole authority to initialize the next slot of one owned tail.
#[must_use]
pub struct Entry<H, T> {
    block: tail::Block<H, mem::MaybeUninit<T>>,
    initialized: usize,
}

impl<H, T> Entry<H, T> {
    pub(in crate::alloc::tail) fn new(
        block: tail::Block<H, mem::MaybeUninit<T>>,
    ) -> ops::ControlFlow<tail::Block<H, T>, Self> {
        if tail::View::new(&block).is_empty() {
            // SAFETY: an empty tail has no outstanding initialization obligation.
            return ops::ControlFlow::Break(unsafe { block.assume_init() });
        }
        ops::ControlFlow::Continue(Self {
            block,
            initialized: 0,
        })
    }

    pub fn header(&self) -> &H {
        tail::View::new(&self.block).header()
    }

    pub const fn index(&self) -> usize {
        self.initialized
    }

    /// Initializes the pending slot, whose index is below the representable extent.
    pub fn insert(mut self, value: T) -> ops::ControlFlow<tail::Block<H, T>, Self> {
        // SAFETY: Entry owns exactly the next uninitialized slot inside its tail.
        unsafe {
            self.block
                .data()
                .add(self.initialized)
                .write(mem::MaybeUninit::new(value))
        };
        self.initialized += 1;
        if self.initialized == tail::View::new(&self.block).len() {
            let this = mem::ManuallyDrop::new(self);
            // SAFETY: consuming the final write transfers the completely initialized owner.
            let block = unsafe { ptr::read(&this.block).assume_init() };
            ops::ControlFlow::Break(block)
        } else {
            ops::ControlFlow::Continue(self)
        }
    }
}

impl<H, T> Drop for Entry<H, T> {
    fn drop(&mut self) {
        let values = ptr::slice_from_raw_parts_mut(self.block.data().cast::<T>(), self.initialized);
        // SAFETY: this owner retains exactly this initialized prefix; its block
        // subsequently drops the header and releases the complete allocation.
        unsafe { ptr::drop_in_place(values) };
    }
}
