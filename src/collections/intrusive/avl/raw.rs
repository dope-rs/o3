use std::{cell, marker, mem, pin, ptr};

type Link<T> = Option<ptr::NonNull<Entry<T>>>;

struct Node<T> {
    left: cell::Cell<Link<T>>,
    right: cell::Cell<Link<T>>,
    parent: cell::Cell<Link<T>>,
    height: cell::Cell<u8>,
    _marker: marker::PhantomData<*mut ()>,
    _pin: marker::PhantomPinned,
}

impl<T> Node<T> {
    const fn new() -> Self {
        Self {
            left: cell::Cell::new(None),
            right: cell::Cell::new(None),
            parent: cell::Cell::new(None),
            height: cell::Cell::new(0),
            _marker: marker::PhantomData,
            _pin: marker::PhantomPinned,
        }
    }
}

/// A structurally pinned AVL node and its owner.
#[repr(C)]
pub struct Entry<T> {
    node: Node<T>,
    value: T,
}

impl<T> Entry<T> {
    pub const fn new(value: T) -> Self {
        Self {
            node: Node::new(),
            value,
        }
    }

    pub fn value(self: pin::Pin<&Self>) -> &T {
        &pin::Pin::get_ref(self).value
    }

    pub fn is_linked(self: pin::Pin<&Self>) -> bool {
        pin::Pin::get_ref(self).node.height.get() != 0
    }
}

pub struct Tree<T> {
    root: cell::Cell<Link<T>>,
    first: cell::Cell<Link<T>>,
    _marker: marker::PhantomData<*mut ()>,
    _value: marker::PhantomData<fn(T) -> T>,
}

const _: () = {
    assert!(mem::size_of::<Tree<usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Entry<usize>>() == 5 * mem::size_of::<usize>());
};

impl<T> Tree<T> {
    pub const fn new() -> Self {
        Self {
            root: cell::Cell::new(None),
            first: cell::Cell::new(None),
            _marker: marker::PhantomData,
            _value: marker::PhantomData,
        }
    }

    pub fn first_entry(&self) -> Option<pin::Pin<&Entry<T>>> {
        self.first.get().map(|entry| {
            // SAFETY: only pinned Entry nodes can enter this typed tree.
            unsafe { pin::Pin::new_unchecked(entry.as_ref()) }
        })
    }

    /// # Safety
    /// The entry must initially be unlinked. It must remain pinned and live,
    /// and its ordering key must not change, until it is removed from this tree.
    pub unsafe fn insert_entry(
        &self,
        entry: pin::Pin<&Entry<T>>,
        mut before: impl FnMut(&T, &T) -> bool,
    ) {
        let entry = ptr::NonNull::from(entry.get_ref());
        // SAFETY: the insertion contract keeps the complete entry live and
        // pinned while any typed link can reach it.
        let entry_ref = unsafe { entry.as_ref() };
        let node_ref = &entry_ref.node;
        debug_assert!(node_ref.left.get().is_none());
        debug_assert!(node_ref.right.get().is_none());
        debug_assert!(node_ref.parent.get().is_none());
        debug_assert_eq!(node_ref.height.get(), 0);
        let mut current = self.root.get();
        let mut parent = None;
        let mut left = false;
        while let Some(existing) = current {
            parent = Some(existing);
            // SAFETY: every tree link points to a live pinned Entry<T>.
            let existing_ref = unsafe { existing.as_ref() };
            left = before(&entry_ref.value, &existing_ref.value);
            current = if left {
                existing_ref.node.left.get()
            } else {
                existing_ref.node.right.get()
            };
        }
        node_ref.parent.set(parent);
        node_ref.height.set(1);
        if let Some(parent) = parent {
            if left {
                unsafe { parent.as_ref() }.node.left.set(Some(entry));
            } else {
                unsafe { parent.as_ref() }.node.right.set(Some(entry));
            }
        } else {
            self.root.set(Some(entry));
        }
        if parent.is_none() || left && self.first.get() == parent {
            self.first.set(Some(entry));
        }
        self.rebalance(parent);
    }

    /// # Safety
    /// Entry is linked in this tree exactly once.
    pub unsafe fn remove_entry(&self, entry: pin::Pin<&Entry<T>>) {
        let entry = ptr::NonNull::from(entry.get_ref());
        // SAFETY: the removal contract names the live pinned entry linked here.
        let node_ref = &unsafe { entry.as_ref() }.node;
        debug_assert_ne!(node_ref.height.get(), 0);
        let left = node_ref.left.get();
        let right = node_ref.right.get();
        let was_first = self.first.get() == Some(entry);
        debug_assert!(!was_first || left.is_none());
        let next_first = if was_first {
            right.map_or(node_ref.parent.get(), |right| {
                Some(unsafe { Self::first_from(right).0 })
            })
        } else {
            None
        };
        let rebalance = match (left, right) {
            (None, replacement) | (replacement, None) => {
                let parent = node_ref.parent.get();
                unsafe { self.transplant(entry, replacement) };
                parent
            }
            (Some(left), Some(right)) => {
                let (successor, above) = unsafe { Self::first_from(right) };
                let successor_ref = unsafe { successor.as_ref() };
                match above {
                    None => {
                        unsafe { self.transplant(entry, Some(successor)) };
                        successor_ref.node.left.set(Some(left));
                        unsafe { left.as_ref() }.node.parent.set(Some(successor));
                        Some(successor)
                    }
                    Some(parent) => {
                        unsafe { self.transplant(successor, successor_ref.node.right.get()) };
                        successor_ref.node.right.set(Some(right));
                        unsafe { right.as_ref() }.node.parent.set(Some(successor));
                        unsafe { self.transplant(entry, Some(successor)) };
                        successor_ref.node.left.set(Some(left));
                        unsafe { left.as_ref() }.node.parent.set(Some(successor));
                        Self::update_height(successor);
                        Some(parent)
                    }
                }
            }
        };
        node_ref.left.set(None);
        node_ref.right.set(None);
        node_ref.parent.set(None);
        node_ref.height.set(0);
        self.rebalance(rebalance);
        if was_first {
            self.first.set(next_first);
        }
    }

    fn height(entry: Link<T>) -> u8 {
        entry.map_or(0, |entry| unsafe { entry.as_ref() }.node.height.get())
    }

    fn update_height(entry: ptr::NonNull<Entry<T>>) {
        let node = &unsafe { entry.as_ref() }.node;
        node.height
            .set(Self::height(node.left.get()).max(Self::height(node.right.get())) + 1);
    }

    fn balance(entry: ptr::NonNull<Entry<T>>) -> i16 {
        let node = &unsafe { entry.as_ref() }.node;
        i16::from(Self::height(node.left.get())) - i16::from(Self::height(node.right.get()))
    }

    unsafe fn transplant(&self, old: ptr::NonNull<Entry<T>>, replacement: Link<T>) {
        let parent = unsafe { old.as_ref() }.node.parent.get();
        if let Some(parent) = parent {
            let parent_ref = &unsafe { parent.as_ref() }.node;
            if parent_ref.left.get() == Some(old) {
                parent_ref.left.set(replacement);
            } else {
                parent_ref.right.set(replacement);
            }
        } else {
            self.root.set(replacement);
        }
        if let Some(replacement) = replacement {
            unsafe { replacement.as_ref() }.node.parent.set(parent);
        }
    }

    unsafe fn rotate_left(
        &self,
        root: ptr::NonNull<Entry<T>>,
        pivot: ptr::NonNull<Entry<T>>,
    ) -> ptr::NonNull<Entry<T>> {
        let root_ref = &unsafe { root.as_ref() }.node;
        let pivot_ref = &unsafe { pivot.as_ref() }.node;
        let middle = pivot_ref.left.get();
        unsafe { self.transplant(root, Some(pivot)) };
        pivot_ref.left.set(Some(root));
        root_ref.parent.set(Some(pivot));
        root_ref.right.set(middle);
        if let Some(middle) = middle {
            unsafe { middle.as_ref() }.node.parent.set(Some(root));
        }
        Self::update_height(root);
        Self::update_height(pivot);
        pivot
    }

    unsafe fn rotate_right(
        &self,
        root: ptr::NonNull<Entry<T>>,
        pivot: ptr::NonNull<Entry<T>>,
    ) -> ptr::NonNull<Entry<T>> {
        let root_ref = &unsafe { root.as_ref() }.node;
        let pivot_ref = &unsafe { pivot.as_ref() }.node;
        let middle = pivot_ref.right.get();
        unsafe { self.transplant(root, Some(pivot)) };
        pivot_ref.right.set(Some(root));
        root_ref.parent.set(Some(pivot));
        root_ref.left.set(middle);
        if let Some(middle) = middle {
            unsafe { middle.as_ref() }.node.parent.set(Some(root));
        }
        Self::update_height(root);
        Self::update_height(pivot);
        pivot
    }

    fn rebalance(&self, mut node: Link<T>) {
        while let Some(current) = node {
            Self::update_height(current);
            let current_ref = &unsafe { current.as_ref() }.node;
            let balance = Self::balance(current);
            let root = if balance > 1
                && let Some(left) = current_ref.left.get()
            {
                let child = if Self::balance(left) < 0
                    && let Some(pivot) = unsafe { left.as_ref() }.node.right.get()
                {
                    unsafe { self.rotate_left(left, pivot) }
                } else {
                    left
                };
                unsafe { self.rotate_right(current, child) }
            } else if balance < -1
                && let Some(right) = current_ref.right.get()
            {
                let child = if Self::balance(right) > 0
                    && let Some(pivot) = unsafe { right.as_ref() }.node.left.get()
                {
                    unsafe { self.rotate_right(right, pivot) }
                } else {
                    right
                };
                unsafe { self.rotate_left(current, child) }
            } else {
                current
            };
            node = unsafe { root.as_ref() }.node.parent.get();
        }
    }

    unsafe fn first_from(node: ptr::NonNull<Entry<T>>) -> (ptr::NonNull<Entry<T>>, Link<T>) {
        let mut current = node;
        let mut parent = None;
        while let Some(left) = unsafe { current.as_ref() }.node.left.get() {
            parent = Some(current);
            current = left;
        }
        (current, parent)
    }
}

impl<T> Default for Tree<T> {
    fn default() -> Self {
        Self::new()
    }
}
