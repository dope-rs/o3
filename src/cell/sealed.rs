use std::cell;

/// Single-threaded storage whose exclusive callback rejects reentry.
///
/// Its borrow flag is restored during unwinding.
#[repr(C)]
pub struct Checked<T> {
    value: cell::UnsafeCell<T>,
    active: cell::Cell<bool>,
    thread: crate::ThreadBound,
}

impl<T> Checked<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: cell::UnsafeCell::new(value),
            active: cell::Cell::new(false),
            thread: crate::ThreadBound::NEW,
        }
    }

    pub fn with_mut<R>(&self, operation: impl for<'a> FnOnce(&'a mut T) -> R) -> R {
        assert!(!self.active.replace(true), "reentrant checked cell access");
        let access = Access(&self.active);
        // SAFETY: the active flag rejects reentry on this thread. ThreadBound
        // prevents moving or sharing the cell with another thread, and Access
        // restores the flag on both normal return and unwinding.
        let result = operation(unsafe { &mut *self.value.get() });
        drop(access);
        result
    }
}

struct Access<'a>(&'a cell::Cell<bool>);

impl Drop for Access<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
