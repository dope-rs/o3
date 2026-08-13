use std::cell::Cell;

use o3::collections::slab::{Capacity, pinned};

use crate::support::{PanicDrop, PinnedItem};

#[test]
fn dynamic_and_fixed_slots_stay_pinned() {
    let drops = Cell::new(0);
    let mut slab: pinned::Pool<PinnedItem<'_>> = pinned::Pool::with_capacity(Capacity::new(2));
    assert!(slab.is_empty());
    let Ok(first) = slab.insert(PinnedItem::new(1, &drops)) else {
        panic!("capacity");
    };
    assert_eq!(slab.len(), 1);
    let first_parts = first.parts();
    assert!(slab.parts(first_parts).is_some());
    assert_eq!(slab.key(first.index()), Some(first));
    slab.parts(first_parts).unwrap().bind();

    let mut moved = slab;
    let (indexed, mut first_value) = moved.index_mut(first.index()).unwrap();
    assert_eq!(indexed, first);
    first_value.as_mut().set(2);
    assert_eq!(moved.get(first).unwrap().value(), 2);
    assert!(moved.remove_parts(first_parts));
    assert!(moved.is_empty());
    assert!(moved.parts(first_parts).is_none());
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
    let mut slab = std::pin::pin!(pinned::Fixed::<PinnedItem<'_>, 2>::new());
    assert!(slab.is_empty());
    let key = slab
        .as_mut()
        .vacant_entry()
        .expect("capacity")
        .insert(PinnedItem::new(4, &drops));
    let parts = key.parts();
    assert_eq!(slab.key(key.index()), Some(key));
    {
        let mut value = pinned::Fixed::parts_mut(slab.as_mut(), parts).unwrap();
        value.as_ref().bind();
        value.as_mut().set(5);
        assert_eq!(value.as_ref().value(), 5);
    }
    assert_eq!(slab.len(), 1);
    assert!(slab.as_mut().remove_parts(parts));
    assert!(slab.is_empty());
    assert_eq!(drops.get(), 1);
}

#[test]
fn exhausted_generations_retire_slots() {
    let drops = Cell::new(0);
    let mut slab = pinned::Pool::<PinnedItem<'_>, (), 1>::with_capacity(Capacity::new(1));
    let Ok(key) = slab.insert(PinnedItem::new(1, &drops)) else {
        panic!("capacity");
    };
    slab.get(key).unwrap().bind();
    assert!(slab.remove(key));
    assert!(slab.insert(PinnedItem::new(2, &drops)).is_err());
}

#[test]
fn vacant_entries_commit_once_and_cancel_without_state_changes() {
    let mut dynamic = pinned::Pool::<u32>::with_capacity(Capacity::new(1));
    {
        let _entry = dynamic
            .vacant_entry()
            .expect("new dynamic pin slab should have one vacant slot");
    }
    let key = dynamic
        .vacant_entry()
        .expect("dropping a vacant entry should leave its slot available")
        .insert(7);
    assert_eq!(dynamic.get(key).map(|value| *value), Some(7));
    assert!(dynamic.vacant_entry().is_none());

    let mut fixed = std::pin::pin!(pinned::Fixed::<u32, 1>::new());
    {
        let _entry = fixed
            .as_mut()
            .vacant_entry()
            .expect("new fixed pin slab should have one vacant slot");
    }
    let key = fixed
        .as_mut()
        .vacant_entry()
        .expect("dropping a fixed vacant entry should leave its slot available")
        .insert(9);
    assert_eq!(
        pinned::Fixed::parts_mut(fixed.as_mut(), key.parts()).map(|value| *value),
        Some(9)
    );
    assert!(fixed.as_mut().vacant_entry().is_none());
}

#[test]
fn drop_panics_do_not_leak_other_slots() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut slab: pinned::Pool<PanicDrop<'_>> = pinned::Pool::with_capacity(Capacity::new(2));
        slab.insert(PanicDrop::new(0, &drops, &panic_once)).ok();
        slab.insert(PanicDrop::new(1, &drops, &panic_once)).ok();
        drop(slab);
    }));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 2);

    drops.set(0);
    panic_once.set(true);
    let mut slab: pinned::Pool<PanicDrop<'_>> = pinned::Pool::with_capacity(Capacity::new(2));
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
