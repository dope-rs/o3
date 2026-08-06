use std::{
    cell::Cell,
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
    ptr::NonNull,
};

struct Node {
    left: Cell<Option<NonNull<Node>>>,
    right: Cell<Option<NonNull<Node>>>,
    parent: Cell<Option<NonNull<Node>>>,
    height: Cell<u8>,
    _marker: PhantomData<*mut ()>,
    _pin: PhantomPinned,
}

impl Node {
    const fn new() -> Self {
        Self {
            left: Cell::new(None),
            right: Cell::new(None),
            parent: Cell::new(None),
            height: Cell::new(0),
            _marker: PhantomData,
            _pin: PhantomPinned,
        }
    }
}

/// A structurally pinned AVL node and its owner.
#[repr(C)]
pub struct Entry<T> {
    node: Node,
    value: T,
}

impl<T> Entry<T> {
    pub const fn new(value: T) -> Self {
        Self {
            node: Node::new(),
            value,
        }
    }

    pub fn value(self: Pin<&Self>) -> &T {
        &Pin::get_ref(self).value
    }

    pub fn is_linked(self: Pin<&Self>) -> bool {
        Pin::get_ref(self).node.height.get() != 0
    }
}

pub struct Tree<T> {
    root: Cell<Option<NonNull<Node>>>,
    first: Cell<Option<NonNull<Node>>>,
    _marker: PhantomData<*mut ()>,
    _value: PhantomData<fn(T) -> T>,
}

impl<T> Tree<T> {
    pub const fn new() -> Self {
        Self {
            root: Cell::new(None),
            first: Cell::new(None),
            _marker: PhantomData,
            _value: PhantomData,
        }
    }

    pub fn first_entry(&self) -> Option<Pin<&Entry<T>>> {
        self.first.get().map(|node| {
            // SAFETY: only pinned Entry nodes can enter this typed tree.
            unsafe { Pin::new_unchecked(self.value(node)) }
        })
    }

    /// # Safety
    /// Entry stays pinned, live, unlinked, and strictly ordered while linked.
    pub unsafe fn insert_entry(
        &self,
        entry: Pin<&Entry<T>>,
        mut before: impl FnMut(&T, &T) -> bool,
    ) {
        let value_ref = entry.get_ref();
        let node = NonNull::from(&value_ref.node);
        let node_ref = unsafe { node.as_ref() };
        debug_assert!(node_ref.left.get().is_none());
        debug_assert!(node_ref.right.get().is_none());
        debug_assert!(node_ref.parent.get().is_none());
        debug_assert_eq!(node_ref.height.get(), 0);
        let mut current = self.root.get();
        let mut parent = None;
        let mut left = false;
        while let Some(existing) = current {
            parent = Some(existing);
            left = before(&value_ref.value, &unsafe { self.value(existing) }.value);
            current = if left {
                unsafe { existing.as_ref() }.left.get()
            } else {
                unsafe { existing.as_ref() }.right.get()
            };
        }
        node_ref.parent.set(parent);
        node_ref.height.set(1);
        if let Some(parent) = parent {
            if left {
                unsafe { parent.as_ref() }.left.set(Some(node));
            } else {
                unsafe { parent.as_ref() }.right.set(Some(node));
            }
        } else {
            self.root.set(Some(node));
        }
        if parent.is_none() || left && self.first.get() == parent {
            self.first.set(Some(node));
        }
        self.rebalance(parent);
    }

    /// # Safety
    /// Entry is linked in this tree exactly once.
    pub unsafe fn remove_entry(&self, entry: Pin<&Entry<T>>) {
        let node = NonNull::from(&entry.get_ref().node);
        let node_ref = unsafe { node.as_ref() };
        debug_assert_ne!(node_ref.height.get(), 0);
        let left = node_ref.left.get();
        let right = node_ref.right.get();
        let was_first = self.first.get() == Some(node);
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
                unsafe { self.transplant(node, replacement) };
                parent
            }
            (Some(left), Some(right)) => {
                let (successor, above) = unsafe { Self::first_from(right) };
                let successor_ref = unsafe { successor.as_ref() };
                match above {
                    None => {
                        unsafe { self.transplant(node, Some(successor)) };
                        successor_ref.left.set(Some(left));
                        unsafe { left.as_ref() }.parent.set(Some(successor));
                        Some(successor)
                    }
                    Some(parent) => {
                        unsafe { self.transplant(successor, successor_ref.right.get()) };
                        successor_ref.right.set(Some(right));
                        unsafe { right.as_ref() }.parent.set(Some(successor));
                        unsafe { self.transplant(node, Some(successor)) };
                        successor_ref.left.set(Some(left));
                        unsafe { left.as_ref() }.parent.set(Some(successor));
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

    unsafe fn value(&self, node: NonNull<Node>) -> &Entry<T> {
        unsafe { node.cast::<Entry<T>>().as_ref() }
    }

    fn height(node: Option<NonNull<Node>>) -> u8 {
        node.map_or(0, |node| unsafe { node.as_ref() }.height.get())
    }

    fn update_height(node: NonNull<Node>) {
        let node = unsafe { node.as_ref() };
        node.height
            .set(Self::height(node.left.get()).max(Self::height(node.right.get())) + 1);
    }

    fn balance(node: NonNull<Node>) -> i16 {
        let node = unsafe { node.as_ref() };
        i16::from(Self::height(node.left.get())) - i16::from(Self::height(node.right.get()))
    }

    unsafe fn transplant(&self, old: NonNull<Node>, replacement: Option<NonNull<Node>>) {
        let parent = unsafe { old.as_ref() }.parent.get();
        if let Some(parent) = parent {
            let parent_ref = unsafe { parent.as_ref() };
            if parent_ref.left.get() == Some(old) {
                parent_ref.left.set(replacement);
            } else {
                parent_ref.right.set(replacement);
            }
        } else {
            self.root.set(replacement);
        }
        if let Some(replacement) = replacement {
            unsafe { replacement.as_ref() }.parent.set(parent);
        }
    }

    unsafe fn rotate_left(&self, root: NonNull<Node>, pivot: NonNull<Node>) -> NonNull<Node> {
        let root_ref = unsafe { root.as_ref() };
        let pivot_ref = unsafe { pivot.as_ref() };
        let middle = pivot_ref.left.get();
        unsafe { self.transplant(root, Some(pivot)) };
        pivot_ref.left.set(Some(root));
        root_ref.parent.set(Some(pivot));
        root_ref.right.set(middle);
        if let Some(middle) = middle {
            unsafe { middle.as_ref() }.parent.set(Some(root));
        }
        Self::update_height(root);
        Self::update_height(pivot);
        pivot
    }

    unsafe fn rotate_right(&self, root: NonNull<Node>, pivot: NonNull<Node>) -> NonNull<Node> {
        let root_ref = unsafe { root.as_ref() };
        let pivot_ref = unsafe { pivot.as_ref() };
        let middle = pivot_ref.right.get();
        unsafe { self.transplant(root, Some(pivot)) };
        pivot_ref.right.set(Some(root));
        root_ref.parent.set(Some(pivot));
        root_ref.left.set(middle);
        if let Some(middle) = middle {
            unsafe { middle.as_ref() }.parent.set(Some(root));
        }
        Self::update_height(root);
        Self::update_height(pivot);
        pivot
    }

    fn rebalance(&self, mut node: Option<NonNull<Node>>) {
        while let Some(current) = node {
            Self::update_height(current);
            let current_ref = unsafe { current.as_ref() };
            let balance = Self::balance(current);
            let root = if balance > 1
                && let Some(left) = current_ref.left.get()
            {
                let child = if Self::balance(left) < 0
                    && let Some(pivot) = unsafe { left.as_ref() }.right.get()
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
                    && let Some(pivot) = unsafe { right.as_ref() }.left.get()
                {
                    unsafe { self.rotate_right(right, pivot) }
                } else {
                    right
                };
                unsafe { self.rotate_left(current, child) }
            } else {
                current
            };
            node = unsafe { root.as_ref() }.parent.get();
        }
    }

    unsafe fn first_from(node: NonNull<Node>) -> (NonNull<Node>, Option<NonNull<Node>>) {
        let mut current = node;
        let mut parent = None;
        while let Some(left) = unsafe { current.as_ref() }.left.get() {
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
