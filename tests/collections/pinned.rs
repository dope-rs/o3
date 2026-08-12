use std::{cell::Cell, mem::size_of, pin::Pin};

use o3::collections::pinned::Slice;

use crate::support::PinnedItem;

#[test]
fn dense_entries_stay_pinned_when_the_owner_moves() {
    let drops = Cell::new(0);
    let entries: Slice<_> = [PinnedItem::new(1, &drops), PinnedItem::new(2, &drops)]
        .into_iter()
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(!entries.is_empty());
    assert_eq!(entries.iter().len(), 2);
    for entry in entries.iter() {
        entry.bind();
    }
    assert!(entries.get(2).is_none());

    let moved = entries;
    assert_eq!(
        moved.iter().map(PinnedItem::value).collect::<Vec<_>>(),
        [1, 2],
    );
    drop(moved);
    assert_eq!(drops.get(), 2);
}

#[test]
fn wrapper_has_the_exact_pinned_box_layout() {
    assert_eq!(
        size_of::<Slice<PinnedItem<'static>>>(),
        size_of::<Pin<Box<[PinnedItem<'static>]>>>(),
    );
}
