//! Unsafe boundary for borrowed resident nodes and their stable hooks.

use std::{cell, mem, ops};

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

    pub fn is_parked(&self) -> bool {
        let hook = self.item.hook();
        hook.prev_index() == Some(self.key) && hook.next_index() == Some(self.key)
    }

    pub fn unpark(self) -> Option<links::Detached<'a>> {
        self.item.hook().try_unpark(self.key)
    }
}

const _: () = {
    assert!(mem::size_of::<View<'static, u64>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<View<'static, u64>>>() == 2 * mem::size_of::<usize>());
};

/// One optionally initialized resident payload.
pub struct Entry<T> {
    value: cell::Cell<mem::MaybeUninit<T>>,
}

impl<T> Default for Entry<T> {
    fn default() -> Self {
        Self {
            value: cell::Cell::new(mem::MaybeUninit::uninit()),
        }
    }
}

impl<T> Entry<T> {
    pub fn initialize(&self, value: T)
    where
        T: Copy,
    {
        self.value.set(mem::MaybeUninit::new(value));
    }

    /// # Safety
    /// The caller owns this exact initialized payload and consumes it once.
    /// No access may read or retire it again before its next initialization.
    pub unsafe fn take(&self) -> T {
        // SAFETY: the owning topology proves this entry's value is initialized.
        unsafe { self.value.replace(mem::MaybeUninit::uninit()).assume_init() }
    }

    /// # Safety
    /// The caller owns this exact initialized payload and its sole drop obligation.
    /// No access may read or retire it again before its next initialization.
    pub unsafe fn retire(&self) {
        let mut value = self.value.replace(mem::MaybeUninit::uninit());
        // SAFETY: the owning topology proves this entry's value is initialized.
        unsafe { value.assume_init_drop() };
    }
}
