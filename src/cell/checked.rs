use std::cell::{Cell, UnsafeCell};

/// Single-threaded storage whose exclusive callback rejects reentry.
///
/// Its borrow flag is restored during unwinding.
#[repr(C)]
pub struct Checked<T> {
    value: UnsafeCell<T>,
    active: Cell<bool>,
    _thread: crate::ThreadBound,
}

impl<T> Checked<T> {
    pub const fn new(value: T) -> Self {
        use crate::ThreadBound;
        Self {
            value: UnsafeCell::new(value),
            active: Cell::new(false),
            _thread: ThreadBound::NEW,
        }
    }

    pub fn with_mut<R>(&self, operation: impl for<'a> FnOnce(&'a mut T) -> R) -> R {
        assert!(!self.active.replace(true), "reentrant checked cell access");
        let _access = Access(&self.active);
        operation(unsafe { &mut *self.value.get() })
    }
}

struct Access<'a>(&'a Cell<bool>);

impl Drop for Access<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
