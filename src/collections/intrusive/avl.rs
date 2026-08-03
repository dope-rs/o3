use std::cell::Cell;
use std::marker::{PhantomData, PhantomPinned};
use std::pin::Pin;
use std::ptr::NonNull;

pub struct AvlNode {
    left: Cell<Option<NonNull<AvlNode>>>,
    right: Cell<Option<NonNull<AvlNode>>>,
    parent: Cell<Option<NonNull<AvlNode>>>,
    height: Cell<u8>,
    _marker: PhantomData<*mut ()>,
    _pin: PhantomPinned,
}

impl AvlNode {
    pub const fn new() -> Self {
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

impl Default for AvlNode {
    fn default() -> Self {
        Self::new()
    }
}

/// A structurally pinned AVL node and its owner.
#[repr(C)]
pub struct AvlEntry<T> {
    node: AvlNode,
    value: T,
}

impl<T> AvlEntry<T> {
    pub const fn new(value: T) -> Self {
        Self {
            node: AvlNode::new(),
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

#[doc(hidden)]
pub struct AvlEntryAdapter<T>(PhantomData<fn(T) -> T>);

// SAFETY: `AvlEntry` is repr(C) with `node` at offset zero. The projection is
// structurally pinned, and casting that node address back is its exact inverse.
unsafe impl<T> AvlAdapter for AvlEntryAdapter<T> {
    type Value = AvlEntry<T>;

    fn node(value: Pin<&Self::Value>) -> Pin<&AvlNode> {
        // SAFETY: `node` is a structurally pinned field of `AvlEntry`.
        unsafe { value.map_unchecked(|entry| &entry.node) }
    }

    unsafe fn from_node(node: NonNull<AvlNode>) -> NonNull<Self::Value> {
        node.cast()
    }
}

pub type AvlEntryTree<T> = AvlTree<AvlEntryAdapter<T>>;

impl<T> AvlEntryTree<T> {
    pub fn first_entry(&self) -> Option<Pin<&AvlEntry<T>>> {
        self.first()
    }

    /// # Safety
    /// Entry stays pinned, live, unlinked, and strictly ordered while linked.
    pub unsafe fn insert_entry(
        &self,
        entry: Pin<&AvlEntry<T>>,
        mut before: impl FnMut(&T, &T) -> bool,
    ) {
        unsafe { self.insert(entry, |left, right| before(&left.value, &right.value)) };
    }

    /// # Safety
    /// Entry is linked in this tree exactly once.
    pub unsafe fn remove_entry(&self, entry: Pin<&AvlEntry<T>>) {
        unsafe { self.remove(entry) };
    }
}

/// Maps one intrusive node field to its pinned owner.
///
/// # Safety
/// `node` must always project the same structurally pinned node from `Value`.
/// `from_node` must be its exact inverse for every node returned by `node`,
/// preserving pointer provenance and owner identity.
pub unsafe trait AvlAdapter {
    type Value;

    fn node(value: Pin<&Self::Value>) -> Pin<&AvlNode>;

    /// # Safety
    /// `node` must have been obtained from this adapter's `node` projection
    /// and its owner must still be live at the original pinned address.
    unsafe fn from_node(node: NonNull<AvlNode>) -> NonNull<Self::Value>;
}

pub struct AvlTree<A: AvlAdapter> {
    root: Cell<Option<NonNull<AvlNode>>>,
    first: Cell<Option<NonNull<AvlNode>>>,
    _marker: PhantomData<*mut ()>,
    _adapter: PhantomData<fn(A) -> A>,
}

impl<A: AvlAdapter> AvlTree<A> {
    pub const fn new() -> Self {
        Self {
            root: Cell::new(None),
            first: Cell::new(None),
            _marker: PhantomData,
            _adapter: PhantomData,
        }
    }

    pub fn first(&self) -> Option<Pin<&A::Value>> {
        self.first.get().map(|node| {
            // SAFETY: only `A::node` values can enter this typed tree, and
            // insertion requires their owners to stay pinned while linked.
            unsafe { Pin::new_unchecked(self.value(node)) }
        })
    }

    /// # Safety
    /// `value`'s node must be unlinked and its owner must stay pinned and live
    /// until removal or until this tree can no longer be accessed. `before`
    /// must define a stable strict ordering for the whole linked interval.
    pub unsafe fn insert(
        &self,
        value: Pin<&A::Value>,
        mut before: impl FnMut(&A::Value, &A::Value) -> bool,
    ) {
        let value_ref = value.get_ref();
        let node = NonNull::from(A::node(value).get_ref());
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
            left = before(value_ref, unsafe { self.value(existing) });
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
    /// `value`'s node must be live and linked in this exact tree, and cannot be
    /// removed twice.
    pub unsafe fn remove(&self, value: Pin<&A::Value>) {
        let node = NonNull::from(A::node(value).get_ref());
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

    unsafe fn value(&self, node: NonNull<AvlNode>) -> &A::Value {
        unsafe { A::from_node(node).as_ref() }
    }

    fn height(node: Option<NonNull<AvlNode>>) -> u8 {
        node.map_or(0, |node| unsafe { node.as_ref() }.height.get())
    }

    fn update_height(node: NonNull<AvlNode>) {
        let node = unsafe { node.as_ref() };
        node.height
            .set(Self::height(node.left.get()).max(Self::height(node.right.get())) + 1);
    }

    fn balance(node: NonNull<AvlNode>) -> i16 {
        let node = unsafe { node.as_ref() };
        i16::from(Self::height(node.left.get())) - i16::from(Self::height(node.right.get()))
    }

    unsafe fn transplant(&self, old: NonNull<AvlNode>, replacement: Option<NonNull<AvlNode>>) {
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

    unsafe fn rotate_left(
        &self,
        root: NonNull<AvlNode>,
        pivot: NonNull<AvlNode>,
    ) -> NonNull<AvlNode> {
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

    unsafe fn rotate_right(
        &self,
        root: NonNull<AvlNode>,
        pivot: NonNull<AvlNode>,
    ) -> NonNull<AvlNode> {
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

    fn rebalance(&self, mut node: Option<NonNull<AvlNode>>) {
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

    unsafe fn first_from(node: NonNull<AvlNode>) -> (NonNull<AvlNode>, Option<NonNull<AvlNode>>) {
        let mut current = node;
        let mut parent = None;
        while let Some(left) = unsafe { current.as_ref() }.left.get() {
            parent = Some(current);
            current = left;
        }
        (current, parent)
    }
}

impl<A: AvlAdapter> Default for AvlTree<A> {
    fn default() -> Self {
        Self::new()
    }
}
