//! Unsafe boundary for borrowed resident nodes and their stable hooks.

use std::{cell, convert::Infallible, mem, ops};

use crate::depot::{self, queue::links};

pub use crate::depot::raw::Node;

/// One resident entry borrowed together with its exact compact identity.
pub struct View<'a, T> {
    item: &'a Node<T>,
    key: links::Index,
}

impl<T> Clone for View<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for View<'_, T> {}

impl<T> ops::Deref for View<'_, T> {
    type Target = Node<T>;

    fn deref(&self) -> &Self::Target {
        self.item
    }
}

impl<'a, T> View<'a, T> {
    /// # Safety
    /// `key` resolves a node in this exact slice.
    pub unsafe fn new(entries: &'a [Node<T>], key: links::Index) -> Self {
        let item = unsafe { entries.get_unchecked(key.get()) };
        Self { item, key }
    }

    /// # Safety
    /// Reachable hooks belong to these entries and form an acyclic vacant chain.
    pub unsafe fn acquire(free: &links::Free, entries: &'a [Node<T>]) -> Option<Self> {
        let key = unsafe { depot::raw::Hooks::acquire(entries, free) }?;
        Some(unsafe { Self::new(entries, key) })
    }

    pub fn key(&self) -> &links::Index {
        &self.key
    }

    pub fn into_ref(self) -> &'a Node<T> {
        self.item
    }

    pub fn unpark(self) -> Option<links::Detached<'a>> {
        self.item.hook().try_unpark(self.key)
    }
}

const _: () = {
    assert!(mem::size_of::<View<'static, u64>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<View<'static, u64>>>() == 2 * mem::size_of::<usize>());
};

union Payload<W, G> {
    value: mem::ManuallyDrop<W>,
    grant: mem::ManuallyDrop<G>,
}

/// Storage for the value or grant selected by the resident node's existing links.
pub struct Entry<W, G = Infallible> {
    payload: cell::Cell<mem::MaybeUninit<Payload<W, G>>>,
}

impl<W, G> Default for Entry<W, G> {
    fn default() -> Self {
        Self {
            payload: cell::Cell::new(mem::MaybeUninit::uninit()),
        }
    }
}

impl<W, G> Entry<W, G> {
    fn payload(&self) -> *mut Payload<W, G> {
        self.payload.as_ptr().cast()
    }

    pub fn initialize(&self, value: W)
    where
        W: Copy,
    {
        // SAFETY: the cell permits writing this field without reading its previous contents.
        unsafe { (&raw mut (*self.payload()).value).write(mem::ManuallyDrop::new(value)) };
    }

    /// # Safety
    /// The caller owns this initialized value and consumes it only once.
    pub unsafe fn take(&self) -> W {
        unsafe { mem::ManuallyDrop::take(&mut (*self.payload()).value) }
    }

    /// # Safety
    /// The caller owns this initialized value and its sole drop obligation.
    pub unsafe fn retire(&self) {
        drop(unsafe { self.take() });
    }

    pub(super) fn write_grant(&self, grant: G) {
        // SAFETY: park removed the value before this grant is installed in the cell.
        unsafe { (&raw mut (*self.payload()).grant).write(mem::ManuallyDrop::new(grant)) };
    }

    /// # Safety
    /// The caller removed the parked grant and owns its single transfer.
    pub(super) unsafe fn take_grant(&self) -> G {
        unsafe { mem::ManuallyDrop::take(&mut (*self.payload()).grant) }
    }
}
