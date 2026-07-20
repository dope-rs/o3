use o3::collections::CellSlab;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn reentrant_remove_observes_a_busy_slot() {
    let slab: CellSlab<u32> = CellSlab::with_capacity(1);
    let key = slab.insert(42).unwrap();
    let value = slab.update(key, |value| {
        assert_eq!(slab.remove(key), None);
        *value
    });
    assert_eq!(value, Some(42));
    assert_eq!(slab.remove(key), Some(42));
}

#[test]
fn update_keeps_the_value_in_its_slot() {
    let slab: CellSlab<u32> = CellSlab::with_capacity(1);
    let (key, inserted) = slab
        .insert_with(42, |key, value| (key, value as *mut u32))
        .unwrap();
    let first = slab.update(key, |value| value as *mut u32).unwrap();
    let second = slab.update(key, |value| value as *mut u32).unwrap();
    assert_eq!(inserted, first);
    assert_eq!(first, second);
}

#[test]
fn conditional_remove_visits_the_slot_once() {
    let slab: CellSlab<u32> = CellSlab::with_capacity(1);
    let key = slab.insert(7).unwrap();
    assert!(slab.remove_with(key, |_| None::<()>).is_none());
    let (value, output) = slab.remove_with(key, |value| Some(*value + 1)).unwrap();
    assert_eq!((value, output), (7, 8));
}

#[test]
fn keys_follow_checked_dense_positions() {
    let slab: CellSlab<u32> = CellSlab::with_capacity(4096);
    let first = slab.insert(1).unwrap();
    let second = slab.insert(2).unwrap();
    let third = slab.insert(3).unwrap();

    let mut keys = slab.keys();
    assert_eq!(keys.next(), Some(first));
    assert_eq!(slab.remove(second), Some(2));
    let fourth = slab.insert(4).unwrap();
    assert_eq!(keys.collect::<Vec<_>>(), [third, fourth]);

    let mut keys = slab.keys();
    slab.update(third, |_| {
        let observed = keys.by_ref().collect::<Vec<_>>();
        assert_eq!(observed, [first, fourth]);
        assert!(
            observed
                .into_iter()
                .all(|key| key.index() < slab.capacity() as u32 && slab.contains_key(key))
        );
    });
}

#[test]
fn panicking_callbacks_restore_the_slot() {
    let slab: CellSlab<u32> = CellSlab::with_capacity(1);
    let key = slab.insert(5).unwrap();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        slab.update(key, |value| {
            *value = 999;
            panic!("update");
        })
    }));
    assert!(caught.is_err());
    assert!(slab.contains_key(key));
    assert_eq!(slab.len(), 1);
    assert_eq!(slab.remove(key), Some(999));

    let key = slab.insert(7).unwrap();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        slab.remove_with(key, |_| -> Option<()> { panic!("remove") });
    }));
    assert!(caught.is_err());
    assert!(slab.contains_key(key));
    assert_eq!(slab.remove(key), Some(7));
}
