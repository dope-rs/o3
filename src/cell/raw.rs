use std::cell::UnsafeCell;

use crate::marker::ThreadBound;

/// Single-threaded interior storage with caller-checked mutable access.
#[repr(transparent)]
pub struct RawCell<T> {
    value: UnsafeCell<T>,
    _thread: ThreadBound,
}

impl<T> RawCell<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            _thread: ThreadBound::NEW,
        }
    }

    /// # Safety
    /// No mutable reference to the stored value may be live while `f` runs.
    pub unsafe fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(unsafe { &*self.value.get() })
    }

    /// # Safety
    /// No reference to the stored value may be live while `f` runs, including
    /// through a reentrant call to this cell.
    pub unsafe fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(unsafe { &mut *self.value.get() })
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }
}
