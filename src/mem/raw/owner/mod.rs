use std::{marker, mem::ManuallyDrop, ptr};

use crate::mem::rc;

mod sealed;
pub(super) use sealed::Sealed;

/// One owning edge to an initialized, thread-local reference domain.
/// Moving the edge never moves its separately retained target.
#[derive(Debug)]
#[repr(transparent)]
pub struct Owner<T: crate::mem::raw::Target<W>, W: crate::mem::raw::Counter = u32> {
    pointer: ptr::NonNull<T>,
    ownership: marker::PhantomData<(T, W)>,
}

impl<T: crate::mem::raw::Target<W>, W: crate::mem::raw::Counter> Unpin for Owner<T, W> {}

impl<T: crate::mem::raw::Target<W>, W: crate::mem::raw::Counter> Owner<T, W> {
    /// # Safety
    /// `pointer` transfers exactly one live edge to its initialized target. The
    /// allocation and its count stay live until every edge is consumed.
    pub const unsafe fn from_raw(pointer: ptr::NonNull<T>) -> Self {
        Self {
            pointer,
            ownership: marker::PhantomData,
        }
    }

    pub const fn get(&self) -> &T {
        // SAFETY: this owner keeps the initialized allocation live for the borrow.
        unsafe { self.pointer.as_ref() }
    }

    pub const fn as_ptr(&self) -> ptr::NonNull<T> {
        self.pointer
    }

    /// Transfers this counted edge without running its finalizer.
    pub fn into_raw(self) -> ptr::NonNull<T> {
        ManuallyDrop::new(self).pointer
    }

    /// # Safety
    /// `pointer` transfers one live initialized target edge for release/finalization.
    /// All borrows relying on it must have ended; never access or release it again.
    pub unsafe fn release_raw(pointer: ptr::NonNull<T>) {
        // SAFETY: the caller transfers this exact target's live counted edge.
        if let rc::Release::Last = unsafe { crate::mem::raw::Edge::<W>::release(pointer.as_ref()) }
        {
            // SAFETY: the last edge transfers the exact allocation to its finalizer.
            unsafe { T::destroy(pointer) };
        }
    }

    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        let pointer = self.pointer;
        // SAFETY: this owner retains one live edge throughout admission.
        unsafe { crate::mem::raw::Edge::<W>::try_retain(self.get()) }?;
        // SAFETY: the successful retain issued exactly this additional edge.
        Ok(unsafe { Self::from_raw(pointer) })
    }
}

impl<T: crate::mem::raw::Target<W>, W: crate::mem::raw::Counter> Drop for Owner<T, W> {
    fn drop(&mut self) {
        // SAFETY: Drop consumes this owner's counted edge exactly once.
        unsafe { Self::release_raw(self.pointer) };
    }
}
