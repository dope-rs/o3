use std::cell::Cell;

use crate::support::{PanicDrop, PinnedItem};
use o3::collections::{FixedPinSlab, PinSlab};

#[test]
fn dynamic_and_fixed_slots_stay_pinned() {
    let drops = Cell::new(0);
    let mut slab: PinSlab<PinnedItem<'_>> = PinSlab::with_capacity(2);
    let Ok(first) = slab.insert(PinnedItem::new(1, &drops)) else {
        panic!("capacity");
    };
    let first_parts = first.parts();
    assert!(slab.contains_parts(first_parts));
    assert_eq!(slab.key(first.index()), Some(first));
    slab.get_parts(first_parts).unwrap().bind();

    let mut moved = slab;
    moved.get_parts_mut(first_parts).unwrap().set(2);
    assert_eq!(moved.get(first).unwrap().value(), 2);
    assert!(moved.remove_parts(first_parts));
    assert!(!moved.contains_parts(first_parts));
    assert_eq!(drops.get(), 1);

    let Ok(second) = moved.insert(PinnedItem::new(3, &drops)) else {
        panic!("capacity");
    };
    moved.get(second).unwrap().bind();
    assert_ne!(first, second);
    assert!(moved.get(first).is_none());
    assert!(moved.remove(second));
    assert_eq!(drops.get(), 2);

    let drops = Cell::new(0);
    let mut slab = std::pin::pin!(FixedPinSlab::<PinnedItem<'_>, 2>::new());
    let Ok(key) = slab.as_mut().insert(PinnedItem::new(4, &drops)) else {
        panic!("capacity");
    };
    let parts = key.parts();
    assert!(slab.contains_parts(parts));
    assert_eq!(slab.key(key.index()), Some(key));
    slab.as_ref().get_parts(parts).unwrap().bind();
    FixedPinSlab::get_parts_mut(slab.as_mut(), parts)
        .unwrap()
        .set(5);
    assert_eq!(slab.as_ref().get(key).unwrap().value(), 5);
    assert!(slab.as_mut().remove_parts(parts));
    assert!(!slab.contains_parts(parts));
    assert_eq!(drops.get(), 1);
}

#[test]
fn exhausted_generations_retire_slots() {
    let drops = Cell::new(0);
    let mut slab = PinSlab::<PinnedItem<'_>, (), 1>::with_capacity(1);
    let Ok(key) = slab.insert(PinnedItem::new(1, &drops)) else {
        panic!("capacity");
    };
    slab.get(key).unwrap().bind();
    assert!(slab.remove(key));
    assert!(slab.is_empty());
    assert!(slab.is_full());
    assert!(slab.insert(PinnedItem::new(2, &drops)).is_err());
}

#[test]
fn unpin_values_can_be_taken() {
    let mut dynamic: PinSlab<u32> = PinSlab::with_capacity(1);
    let key = dynamic.insert(7u32).unwrap();
    assert_eq!(dynamic.take(key), Some(7));

    let mut fixed = std::pin::pin!(FixedPinSlab::<u32, 1>::new());
    let key = fixed.as_mut().insert(9).unwrap();
    assert_eq!(fixed.as_mut().take(key), Some(9));
}

#[test]
fn vacant_entries_commit_once_and_cancel_without_state_changes() {
    let mut dynamic = PinSlab::<u32>::with_capacity(1);
    {
        let entry = dynamic
            .vacant_entry()
            .expect("new dynamic pin slab should have one vacant slot");
        assert_eq!(entry.index(), 0);
        assert_eq!(entry.key().index(), 0);
    }
    let key = dynamic
        .vacant_entry()
        .expect("dropping a vacant entry should leave its slot available")
        .insert(7);
    assert_eq!(dynamic.get(key).map(|value| *value), Some(7));
    assert!(dynamic.vacant_entry().is_none());

    let mut fixed = std::pin::pin!(FixedPinSlab::<u32, 1>::new());
    {
        let entry = fixed
            .as_mut()
            .vacant_entry()
            .expect("new fixed pin slab should have one vacant slot");
        assert_eq!(entry.index(), 0);
        assert_eq!(entry.key().index(), 0);
    }
    let key = fixed
        .as_mut()
        .vacant_entry()
        .expect("dropping a fixed vacant entry should leave its slot available")
        .insert(9);
    assert_eq!(fixed.as_ref().get(key).map(|value| *value), Some(9));
    assert!(fixed.as_mut().vacant_entry().is_none());
}

#[test]
fn drop_panics_do_not_leak_other_slots() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut slab: PinSlab<PanicDrop<'_>> = PinSlab::with_capacity(2);
        slab.insert(PanicDrop::new(0, &drops, &panic_once)).ok();
        slab.insert(PanicDrop::new(1, &drops, &panic_once)).ok();
        drop(slab);
    }));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 2);

    drops.set(0);
    panic_once.set(true);
    let mut slab: PinSlab<PanicDrop<'_>> = PinSlab::with_capacity(2);
    let Ok(key) = slab.insert(PanicDrop::new(0, &drops, &panic_once)) else {
        panic!("capacity");
    };
    slab.insert(PanicDrop::new(1, &drops, &panic_once)).ok();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slab.remove(key);
    }));
    assert!(caught.is_err());
    let Ok(replacement) = slab.insert(PanicDrop::new(2, &drops, &panic_once)) else {
        panic!("capacity");
    };
    assert_ne!(key, replacement);
    drop(slab);
    assert_eq!(drops.get(), 3);
}
