use std::collections::BTreeSet;
use std::pin::Pin;
use std::ptr::NonNull;

use o3::collections::intrusive::{AvlAdapter, AvlNode, AvlTree};

#[repr(C)]
struct Entry {
    node: AvlNode,
    key: usize,
}

impl Entry {
    fn new(key: usize) -> Self {
        Self {
            node: AvlNode::new(),
            key,
        }
    }
}

struct EntryAdapter;

// SAFETY: Entry is repr(C), `node` is its leading field, and the projection is
// structural. Casting that field pointer back recovers the exact pinned Entry.
unsafe impl AvlAdapter for EntryAdapter {
    type Value = Entry;

    fn node(value: Pin<&Self::Value>) -> Pin<&AvlNode> {
        unsafe { value.map_unchecked(|entry| &entry.node) }
    }

    unsafe fn from_node(node: NonNull<AvlNode>) -> NonNull<Entry> {
        node.cast()
    }
}

#[cfg(debug_assertions)]
#[test]
fn linked_root_is_not_mistaken_for_an_unlinked_node() {
    let tree = AvlTree::<EntryAdapter>::new();
    let entry = Box::pin(Entry::new(1));
    unsafe { tree.insert(entry.as_ref(), |_, _| false) };

    let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        tree.insert(entry.as_ref(), |_, _| false);
    }));

    assert!(duplicate.is_err());
    unsafe { tree.remove(entry.as_ref()) };
    assert!(tree.first().is_none());
}

#[test]
fn arbitrary_removal_preserves_sorted_minimum() {
    const LEN: usize = 1024;
    let tree = AvlTree::<EntryAdapter>::new();
    let mut entries: Vec<_> = (0..LEN).map(|key| Box::pin(Entry::new(key))).collect();
    let mut order: Vec<_> = (0..LEN).collect();
    for index in 0..LEN {
        let swap = (index * 37 + 17) % LEN;
        order.swap(index, swap);
    }
    for &index in &order {
        unsafe {
            tree.insert(entries[index].as_ref(), |left, right| left.key < right.key);
        }
    }

    let mut expected: BTreeSet<_> = (0..LEN).collect();
    for index in (0..LEN).step_by(3) {
        unsafe { tree.remove(entries[index].as_ref()) };
        expected.remove(&index);
    }
    while let Some(&key) = expected.first() {
        let first = tree
            .first()
            .expect("tree must remain nonempty while expected keys remain");
        assert_eq!(first.key, key);
        unsafe { tree.remove(first) };
        expected.remove(&key);
    }
    assert!(tree.first().is_none());

    for index in (0..LEN).step_by(3) {
        unsafe {
            tree.insert(entries[index].as_ref(), |left, right| left.key < right.key);
        }
    }
    for key in (0..LEN).step_by(3) {
        let first = tree
            .first()
            .expect("tree must contain every key reinserted for removal");
        assert_eq!(first.key, key);
        unsafe { tree.remove(first) };
    }
    assert!(tree.first().is_none());
    entries.clear();
}
