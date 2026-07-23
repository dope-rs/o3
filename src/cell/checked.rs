use std::cell::{Cell, UnsafeCell};

use crate::marker::ThreadBound;

/// Single-threaded interior storage with checked exclusive access.
///
/// The borrow flag prevents a synchronous callback from reentering the cell
/// while its value is mutably borrowed. It is restored during unwinding.
#[repr(C)]
pub struct CheckedCell<T> {
    value: UnsafeCell<T>,
    active: Cell<bool>,
    _thread: ThreadBound,
}

impl<T> CheckedCell<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            active: Cell::new(false),
            _thread: ThreadBound::NEW,
        }
    }

    #[inline]
    pub fn with_mut<R>(&self, operation: impl for<'a> FnOnce(&'a mut T) -> R) -> R {
        assert!(!self.active.replace(true), "reentrant checked cell access");
        let _access = Access(&self.active);
        operation(unsafe { &mut *self.value.get() })
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
}

struct Access<'a>(&'a Cell<bool>);

impl Drop for Access<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
