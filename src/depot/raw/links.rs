use std::{cell, mem, pin, ptr};

use crate::depot;

/// Intrusive list links without length tracking.
pub struct Links<T> {
    head: cell::Cell<Option<ptr::NonNull<depot::raw::Entry<T>>>>,
    tail: cell::Cell<Option<ptr::NonNull<depot::raw::Entry<T>>>>,
}

impl<T> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Links<T> {
    pub const fn new() -> Self {
        Self {
            head: cell::Cell::new(None),
            tail: cell::Cell::new(None),
        }
    }

    pub fn front(&self) -> Option<pin::Pin<&depot::raw::Entry<T>>> {
        self.head.get().map(|entry| {
            // SAFETY: linked entries remain pinned and live until removal.
            unsafe { pin::Pin::new_unchecked(entry.as_ref()) }
        })
    }

    pub fn is_empty(&self) -> bool {
        self.head.get().is_none()
    }

    /// Links one pinned entry at the back.
    /// # Safety
    /// `entry` is unlinked; keep it pinned and live through removal and all borrows returned by `front`.
    pub unsafe fn push_back(&self, entry: pin::Pin<&depot::raw::Entry<T>>) {
        let node = entry.get_ref();
        let entry = ptr::NonNull::from(node);
        let previous = self.tail.get();
        node.previous.set(previous);
        node.next.set(None);
        if let Some(previous) = previous {
            unsafe { previous.as_ref() }.next.set(Some(entry));
        } else {
            self.head.set(Some(entry));
        }
        self.tail.set(Some(entry));
    }

    /// Unlinks without moving the value; insertion overwrites the unread detached links.
    /// # Safety
    /// `entry` is currently linked in this exact list.
    pub unsafe fn remove(&self, entry: pin::Pin<&depot::raw::Entry<T>>) {
        let node = entry.get_ref();
        let previous = node.previous.get();
        let next = node.next.get();
        if let Some(previous) = previous {
            unsafe { previous.as_ref() }.next.set(next);
        } else {
            self.head.set(next);
        }
        if let Some(next) = next {
            unsafe { next.as_ref() }.previous.set(previous);
        } else {
            self.tail.set(previous);
        }
    }
}

const _: () = assert!(mem::size_of::<Links<usize>>() == 2 * mem::size_of::<usize>());
