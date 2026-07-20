use std::cell::Cell;

use o3::collections::{CellSlab, Slab};

enum Short {}

#[test]
fn generations_advance_and_retired_heads_are_skipped() {
    let mut slab: Slab<u32, Short, 3> = Slab::with_capacity(2);
    let first = slab.insert_at_with(0, |_| 1).unwrap();
    assert_eq!(first.generation().get(), 1);
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.insert_at_with(0, |_| 2).unwrap();
    assert_eq!(second.generation().get(), 2);
    assert_eq!(slab.get(first), None);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.insert_at_with(0, |_| 3).unwrap();
    assert_eq!(third.generation().get(), 3);
    assert_eq!(slab.remove(third), Some(3));
    assert_eq!(slab.insert(4).unwrap().index(), 1);

    let slab: CellSlab<u32, Short, 3> = CellSlab::with_capacity(2);
    let first = slab.insert(1).unwrap();
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.insert(2).unwrap();
    assert_eq!(second.generation().get(), 2);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.insert(3).unwrap();
    assert_eq!(third.generation().get(), 3);
    assert_eq!(slab.remove(third), Some(3));
    assert!(!slab.contains_key(first));
    assert!(!slab.contains_key(second));
    assert_eq!(slab.insert(4).unwrap().index(), 1);
}

#[test]
fn constructor_unwind_invalidates_exposed_keys() {
    let mut slab: Slab<u32> = Slab::with_capacity(1);
    let exposed = Cell::new(None);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slab.insert_at_with(0, |key| {
            exposed.set(Some(key));
            panic!("constructor");
        });
    }));
    assert!(caught.is_err());
    let stale = exposed.get().unwrap();
    let fresh = slab.insert_at_with(0, |_| 7).unwrap();
    assert_ne!(stale, fresh);
    assert_eq!(slab.get(stale), None);
    assert_eq!(slab.get(fresh), Some(&7));

    let slab: CellSlab<u32> = CellSlab::with_capacity(1);
    let exposed = Cell::new(None);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = slab.insert_with(1, |key, _| {
            exposed.set(Some(key));
            panic!("initializer");
        });
    }));
    assert!(caught.is_err());
    let stale = exposed.get().unwrap();
    let fresh = slab.insert(2).unwrap();
    assert_ne!(stale, fresh);
    assert!(!slab.contains_key(stale));
    assert_eq!(slab.remove(stale), None);
    assert_eq!(slab.remove(fresh), Some(2));
}

#[test]
fn reservation_rollback_advances_or_retires_generations() {
    let mut slab: Slab<u32, Short, 3> = Slab::with_capacity(1);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slab.insert_at_with(0, |_| panic!("constructor"));
    }));
    assert!(caught.is_err());
    let second = slab.insert_at_with(0, |_| 2).unwrap();
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.insert_at_with(0, |_| 3).unwrap();
    assert_eq!(slab.remove(third), Some(3));
    assert!(slab.insert(4).is_err());
    assert!(slab.is_full());

    let mut slab: Slab<u32> = Slab::with_capacity(1);
    let reservation = slab.vacant_entry().unwrap();
    let cancelled = reservation.key();
    drop(reservation);
    let fresh = slab.insert(7).unwrap();
    assert_ne!(fresh, cancelled);
    assert_eq!(slab.get(fresh), Some(&7));
}
