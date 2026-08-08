use std::collections::BTreeSet;

use o3::collections::intrusive::avl::{Entry, Tree};

#[test]
fn linked_root_is_not_mistaken_for_an_unlinked_node() {
    let tree = Tree::new();
    let entry = Box::pin(Entry::new(1));
    unsafe { tree.insert_entry(entry.as_ref(), |_, _| false) };

    assert!(entry.as_ref().is_linked());
    assert_eq!(tree.first_entry().map(|entry| *entry.value()), Some(1));
    unsafe { tree.remove_entry(entry.as_ref()) };
    assert!(!entry.as_ref().is_linked());
    assert!(tree.first_entry().is_none());
}

#[test]
fn arbitrary_removal_preserves_sorted_minimum() {
    const LEN: usize = 1024;
    let tree = Tree::new();
    let mut entries: Vec<_> = (0..LEN).map(|key| Box::pin(Entry::new(key))).collect();
    let mut order: Vec<_> = (0..LEN).collect();
    for index in 0..LEN {
        let swap = (index * 37 + 17) % LEN;
        order.swap(index, swap);
    }
    for &index in &order {
        unsafe {
            tree.insert_entry(entries[index].as_ref(), |left, right| left < right);
        }
    }

    let mut expected: BTreeSet<_> = (0..LEN).collect();
    for index in (0..LEN).step_by(3) {
        unsafe { tree.remove_entry(entries[index].as_ref()) };
        expected.remove(&index);
    }
    while let Some(&key) = expected.first() {
        let first = tree
            .first_entry()
            .expect("tree must remain nonempty while expected keys remain");
        assert_eq!(*first.value(), key);
        unsafe { tree.remove_entry(first) };
        expected.remove(&key);
    }
    assert!(tree.first_entry().is_none());

    for index in (0..LEN).step_by(3) {
        unsafe {
            tree.insert_entry(entries[index].as_ref(), |left, right| left < right);
        }
    }
    for key in (0..LEN).step_by(3) {
        let first = tree
            .first_entry()
            .expect("tree must contain every key reinserted for removal");
        assert_eq!(*first.value(), key);
        unsafe { tree.remove_entry(first) };
    }
    assert!(tree.first_entry().is_none());
    entries.clear();
}
