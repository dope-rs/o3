use std::{ops, pin, ptr};

use crate::base::marker::ThreadBound;

/// Owns one initialized value while exclusively borrowing its backing storage.
///
/// Observations borrow this owner. Forgetting it leaks the value but leaves the
/// backing uninitialized from its owner's perspective, as with `MaybeUninit`.
#[derive(Debug)]
#[repr(transparent)]
pub struct Occupied<'a, T> {
    pub(super) value: &'a mut T,
    pub(super) bound: ThreadBound,
}

impl<'a, T> Occupied<'a, T> {
    pub fn get_mut(&mut self) -> &mut T {
        self.value
    }

    /// Moves out the initialized value, releasing its borrowed backing.
    pub fn into_inner(self) -> T {
        self.take().0
    }

    /// Moves out the value while retaining its exact vacant backing.
    pub fn take(self) -> (T, super::Vacant<'a, T>) {
        let vacant = self.into_uninit();
        // SAFETY: into_uninit retains the initialized bytes without their destructor.
        // Reading transfers the value once; the vacant phase cannot drop it.
        (unsafe { vacant.value.assume_init_read() }, vacant)
    }

    /// Returns the same backing without reading or dropping the stored value.
    pub fn into_uninit(self) -> super::Vacant<'a, T> {
        // SAFETY: both transparent phases retain the same exclusive pointer and
        // lifetime. The backing has no independent T drop or surviving pin obligation.
        unsafe { std::mem::transmute(self) }
    }

    /// Pins this initialized value until the callback returns, then drops it in place.
    /// The consumed owner retains destruction even if the callback forgets its pin.
    pub fn with_pin<R>(self, run: impl for<'p> FnOnce(pin::Pin<&'p mut T>) -> R) -> R {
        // SAFETY: this owner retains the initialized slot and its exclusive loan.
        // The callback cannot return its borrow, and this frame drops the value at
        // the same address before releasing the backing storage.
        run(unsafe { pin::Pin::new_unchecked(&mut *self.value) })
    }
}

impl<T> ops::Deref for Occupied<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}

impl<T> Drop for Occupied<'_, T> {
    fn drop(&mut self) {
        // SAFETY: this affine owner consumes its exact initialized value once.
        // The borrowed MaybeUninit backing has no independent T destructor.
        unsafe { ptr::drop_in_place(self.value) };
    }
}
