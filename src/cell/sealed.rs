use std::{cell, pin, process, ptr};

/// A non-atomic reference count with an explicit terminal transition.
#[repr(transparent)]
pub struct LocalRefCount(cell::Cell<u32>);

impl LocalRefCount {
    pub const fn empty() -> Self {
        Self(cell::Cell::new(0))
    }

    pub const fn one() -> Self {
        Self(cell::Cell::new(1))
    }

    pub fn is_empty(&self) -> bool {
        self.0.get() == 0
    }

    pub fn is_unique(&self) -> bool {
        self.0.get() == 1
    }

    /// Activates an empty count with one reference.
    ///
    /// Aborts if the count is already active.
    pub fn activate(&self) {
        if !self.try_activate() {
            process::abort();
        }
    }

    /// Activates an empty count, returning `false` if it is already active.
    pub fn try_activate(&self) -> bool {
        if !self.is_empty() {
            return false;
        }
        self.0.set(1);
        true
    }

    /// Deactivates a uniquely held count.
    ///
    /// Aborts unless exactly one terminal reference remains.
    pub fn deactivate(&self) {
        if !self.try_deactivate() {
            process::abort();
        }
    }

    /// Deactivates a uniquely held count, returning `false` otherwise.
    pub fn try_deactivate(&self) -> bool {
        if !self.is_unique() {
            return false;
        }
        self.0.set(0);
        true
    }

    /// Adds one reference.
    ///
    /// Aborts if the count is empty or exhausted.
    pub fn retain(&self) {
        if !self.try_retain() {
            process::abort();
        }
    }

    /// Adds one reference, returning `false` if the count is empty or
    /// exhausted.
    pub fn try_retain(&self) -> bool {
        let current = self.0.get();
        let Some(next) = current.checked_add(1) else {
            return false;
        };
        if current == 0 {
            return false;
        }
        self.0.set(next);
        true
    }

    /// Releases once; `true` leaves a terminal reference to deactivate.
    #[must_use]
    pub fn release(&self) -> bool {
        let Some(last) = self.try_release() else {
            process::abort();
        };
        last
    }

    /// Returns `None` when empty and `Some(true)` for the terminal reference.
    #[must_use]
    pub fn try_release(&self) -> Option<bool> {
        let current = self.0.get();
        if current == 0 {
            return None;
        }
        if current == 1 {
            return Some(true);
        }
        self.0.set(current - 1);
        Some(false)
    }
}

const _: () = assert!(size_of::<LocalRefCount>() == size_of::<u32>());

/// A retained pointer to a structurally pinned value.
///
/// The link has the exact layout of one non-null pointer. Its owner is
/// responsible for revoking every copy before the target can move or drop.
/// The reference returned by [`get`](Self::get) cannot outlive the link borrow.
///
/// ```compile_fail
/// use std::pin::Pin;
/// use o3::cell::StableLink;
///
/// fn widen<'link, 'target, T>(link: &'link StableLink<T>) -> Pin<&'target T> {
///     link.get()
/// }
/// ```
#[repr(transparent)]
pub struct StableLink<T> {
    pointer: ptr::NonNull<T>,
}

impl<T> StableLink<T> {
    pub fn from_stable(source: impl crate::cell::raw::StableLinkSource<T>) -> Self {
        Self {
            pointer: source.pointer(),
        }
    }

    pub fn get(&self) -> pin::Pin<&T> {
        // SAFETY: StableLinkSource guarantees that the target remains pinned
        // and live whenever this link can be borrowed. The returned reference
        // cannot outlive that borrow.
        unsafe { pin::Pin::new_unchecked(self.pointer.as_ref()) }
    }
}

impl<T> Clone for StableLink<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for StableLink<T> {}

impl<T> PartialEq for StableLink<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pointer == other.pointer
    }
}

impl<T> Eq for StableLink<T> {}

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
