use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::support::{PanicDrop, PinnedItem};
use o3::collections::PinCellSlab;

#[test]
fn entries_pin_and_remove_values() {
    let drops = Cell::new(0);
    let slab = std::pin::pin!(PinCellSlab::<PinnedItem<'_>>::with_capacity(2));
    let Ok(first) = slab.as_ref().insert(PinnedItem::new(1, &drops)) else {
        panic!("capacity");
    };
    let slab_ref = slab.as_ref();
    let mut entry = slab_ref.entry(first).unwrap();
    assert!(slab_ref.entry(first).is_none());
    let Ok(second) = slab_ref.insert(PinnedItem::new(2, &drops)) else {
        panic!("capacity");
    };
    {
        let mut item = entry.get_pin_mut();
        item.as_ref().bind();
        item.as_mut().set(3);
        assert_eq!(item.as_ref().value(), 3);
    }
    entry.remove();
    assert_eq!(drops.get(), 1);
    assert!(!slab_ref.is_full());
    let Ok(third) = slab_ref.insert(PinnedItem::new(4, &drops)) else {
        panic!("capacity");
    };
    assert!(slab_ref.remove(second));
    assert!(slab_ref.remove(third));
    assert_eq!(drops.get(), 3);
    assert!(slab_ref.is_empty());
}

#[test]
fn panics_restore_access_and_reclaim_capacity() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let slab = std::pin::pin!(PinCellSlab::<PanicDrop<'_>>::with_capacity(1));
    let key = slab
        .as_ref()
        .vacant_entry()
        .unwrap()
        .insert(PanicDrop::new(0, &drops, &panic_once));

    assert!(slab.as_ref().contains_key(key));

    let caught = catch_unwind(AssertUnwindSafe(|| {
        slab.as_ref().remove(key);
    }));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 1);
    let Ok(next) = slab.as_ref().insert(PanicDrop::new(1, &drops, &panic_once)) else {
        panic!("capacity");
    };
    assert_eq!(next.generation().get(), 2);
    assert!(slab.as_ref().remove(next));
    assert_eq!(drops.get(), 2);
}

#[test]
fn exhausted_generations_retire_slots() {
    let slab = std::pin::pin!(PinCellSlab::<u32, (), 1>::with_capacity(1));
    let key = slab.as_ref().vacant_entry().unwrap().insert(7);
    assert!(slab.as_ref().remove(key));
    assert!(slab.is_empty());
    assert!(slab.is_full());
    assert!(slab.as_ref().vacant_entry().is_none());
}

#[test]
fn cancelled_vacancies_restore_capacity_without_burning_generations() {
    let slab = Box::pin(PinCellSlab::<u32>::with_capacity(1));
    let entry = slab.as_ref().vacant_entry().unwrap();
    assert_eq!(entry.index(), 0);
    drop(entry);

    let caught = catch_unwind(AssertUnwindSafe(|| {
        let _entry = slab.as_ref().vacant_entry().unwrap();
        panic!("vacant entry");
    }));
    assert!(caught.is_err());
    let key = slab.as_ref().insert(7).unwrap();
    assert_eq!(key.generation().get(), 1);
    assert!(slab.as_ref().remove(key));
}

#[test]
fn unwinding_updates_restore_the_slot() {
    let slab = Box::pin(PinCellSlab::<u32>::with_capacity(1));
    let key = slab.as_ref().insert(1).unwrap();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        slab.as_ref().update(key, |_| panic!("update"));
    }));
    assert!(caught.is_err());
    assert!(slab.as_ref().remove_if(key, |value| *value == 1));
}
