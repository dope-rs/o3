use std::{cell, pin, process, ptr};

/// A non-atomic reference count for single-threaded ownership graphs.
///
/// A final [`release`](Self::release) leaves the count active at one so the
/// caller can complete its terminal ownership transition before calling
/// [`deactivate`](Self::deactivate).
#[repr(transparent)]
pub struct LocalRefCount(cell::Cell<u32>);

impl LocalRefCount {
    pub const fn empty() -> Self {
        Self(cell::Cell::new(0))
    }

    pub const fn one() -> Self {
        Self(cell::Cell::new(1))
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.get() == 0
    }

    #[inline]
    pub fn is_unique(&self) -> bool {
        self.0.get() == 1
    }

    /// Activates an empty count with one reference.
    ///
    /// Aborts if the count is already active.
    #[inline]
    pub fn activate(&self) {
        if !self.try_activate() {
            process::abort();
        }
    }

    /// Activates an empty count, returning `false` if it is already active.
    #[inline]
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
    #[inline]
    pub fn deactivate(&self) {
        if !self.try_deactivate() {
            process::abort();
        }
    }

    /// Deactivates a uniquely held count, returning `false` otherwise.
    #[inline]
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
    #[inline]
    pub fn retain(&self) {
        if !self.try_retain() {
            process::abort();
        }
    }

    /// Adds one reference, returning `false` if the count is empty or
    /// exhausted.
    #[inline]
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

    /// Releases one reference and returns whether the terminal reference
    /// remains.
    ///
    /// Aborts if the count is empty. A `true` result must be followed by
    /// [`deactivate`](Self::deactivate) after the caller completes its
    /// terminal ownership transition.
    #[must_use]
    #[inline]
    pub fn release(&self) -> bool {
        let Some(last) = self.try_release() else {
            process::abort();
        };
        last
    }

    /// Releases one reference, returning `None` if the count is empty.
    ///
    /// `Some(true)` leaves the terminal reference active at one; the caller
    /// must subsequently deactivate it.
    #[must_use]
    #[inline]
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

/// Owner proof for a retained pinned link.
///
/// # Safety
/// `pointer` must identify a valid, initialized `T` that remains pinned and
/// live while any link constructed from this source can be accessed. Before
/// the target can move or drop, every copied link must be destroyed or made
/// inaccessible.
pub unsafe trait StableLinkSource<T> {
    fn pointer(self) -> ptr::NonNull<T>;
}

impl<T> StableLink<T> {
    pub fn from_stable(source: impl StableLinkSource<T>) -> Self {
        Self {
            pointer: source.pointer(),
        }
    }

    #[inline]
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
