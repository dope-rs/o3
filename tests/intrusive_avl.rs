use std::collections::BTreeSet;
use std::pin::Pin;
use std::ptr::NonNull;

use o3::collections::intrusive::{AvlNode, AvlTree};

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

    fn node(self: Pin<&Self>) -> Pin<&AvlNode> {
        unsafe { self.map_unchecked(|entry| &entry.node) }
    }

    unsafe fn key(node: NonNull<AvlNode>) -> usize {
        unsafe { node.cast::<Entry>().as_ref() }.key
    }
}

#[cfg(debug_assertions)]
#[test]
fn linked_root_is_not_mistaken_for_an_unlinked_node() {
    let tree = AvlTree::new();
    let entry = Box::pin(Entry::new(1));
    unsafe { tree.insert(entry.as_ref().node(), |_, _| false) };

    let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        tree.insert(entry.as_ref().node(), |_, _| false);
    }));

    assert!(duplicate.is_err());
    unsafe { tree.remove(NonNull::from(entry.as_ref().node().get_ref())) };
    assert!(tree.first().is_none());
}

#[test]
fn arbitrary_removal_preserves_sorted_minimum() {
    const LEN: usize = 1024;
    let tree = AvlTree::new();
    let mut entries: Vec<_> = (0..LEN).map(|key| Box::pin(Entry::new(key))).collect();
    let mut order: Vec<_> = (0..LEN).collect();
    for index in 0..LEN {
        let swap = (index * 37 + 17) % LEN;
        order.swap(index, swap);
    }
    for &index in &order {
        unsafe {
            tree.insert(entries[index].as_ref().node(), |left, right| {
                Entry::key(left) < Entry::key(right)
            });
        }
    }

    let mut expected: BTreeSet<_> = (0..LEN).collect();
    for index in (0..LEN).step_by(3) {
        unsafe { tree.remove(NonNull::from(entries[index].as_ref().node().get_ref())) };
        expected.remove(&index);
    }
    while let Some(&key) = expected.first() {
        let first = tree
            .first()
            .expect("tree must remain nonempty while expected keys remain");
        assert_eq!(unsafe { Entry::key(first) }, key);
        unsafe { tree.remove(first) };
        expected.remove(&key);
    }
    assert!(tree.first().is_none());

    for index in (0..LEN).step_by(3) {
        unsafe {
            tree.insert(entries[index].as_ref().node(), |left, right| {
                Entry::key(left) < Entry::key(right)
            });
        }
    }
    for key in (0..LEN).step_by(3) {
        let first = tree
            .first()
            .expect("tree must contain every key reinserted for removal");
        assert_eq!(unsafe { Entry::key(first) }, key);
        unsafe { tree.remove(first) };
    }
    assert!(tree.first().is_none());
    entries.clear();
}
