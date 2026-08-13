use std::cell::Cell;

use o3::collections::slab::{self, Capacity, Exclusive};

enum Short {}

#[test]
fn generations_advance_and_retired_heads_are_skipped() {
    let mut slab: Exclusive<u32, Short, 3> = Exclusive::with_capacity(Capacity::new(2));
    let first = slab.slots().vacant_entry_at(0).unwrap().insert(1);
    assert_eq!(first.generation().get(), 1);
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.slots().vacant_entry_at(0).unwrap().insert(2);
    assert_eq!(second.generation().get(), 2);
    assert_eq!(slab.get(first), None);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.slots().vacant_entry_at(0).unwrap().insert(3);
    assert_eq!(third.generation().get(), 3);
    assert_eq!(slab.remove(third), Some(3));
    assert_eq!(slab.available(), 1);
    assert_eq!(slab.insert(4).unwrap().index(), 1);
    assert_eq!(slab.available(), 0);

    let slab: slab::Cell<u32, Short, 3> = slab::Cell::with_capacity(Capacity::new(2));
    let first = slab.insert(1).unwrap();
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.insert(2).unwrap();
    assert_eq!(second.generation().get(), 2);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.insert(3).unwrap();
    assert_eq!(third.generation().get(), 3);
    assert_eq!(slab.remove(third), Some(3));
    assert_eq!(slab.available(), 1);
    assert_eq!(slab.update(first, |_| ()), None);
    assert_eq!(slab.update(second, |_| ()), None);
    assert_eq!(slab.insert(4).unwrap().index(), 1);
    assert_eq!(slab.available(), 0);
}

#[test]
fn constructor_unwind_invalidates_exposed_keys() {
    let mut slab: Exclusive<u32> = Exclusive::with_capacity(Capacity::new(1));
    let exposed = Cell::new(None);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let reservation = slab.slots().vacant_entry_at(0).unwrap();
        exposed.set(Some(reservation.key()));
        panic!("constructor");
    }));
    assert!(caught.is_err());
    let stale = exposed.get().unwrap();
    let fresh = slab.slots().vacant_entry_at(0).unwrap().insert(7);
    assert_ne!(stale, fresh);
    assert_eq!(slab.get(stale), None);
    assert_eq!(slab.get(fresh), Some(&7));
}

#[test]
fn reservation_rollback_advances_or_retires_generations() {
    let mut slab: Exclusive<u32, Short, 3> = Exclusive::with_capacity(Capacity::new(1));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _reservation = slab.slots().vacant_entry_at(0).unwrap();
        panic!("constructor");
    }));
    assert!(caught.is_err());
    let second = slab.slots().vacant_entry_at(0).unwrap().insert(2);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.slots().vacant_entry_at(0).unwrap().insert(3);
    assert_eq!(slab.remove(third), Some(3));
    assert!(slab.insert(4).is_err());
    assert!(slab.is_full());
    assert_eq!(slab.available(), 0);

    let mut slab: Exclusive<u32> = Exclusive::with_capacity(Capacity::new(1));
    let reservation = slab.vacant_entry().unwrap();
    let cancelled = reservation.key();
    drop(reservation);
    let fresh = slab.insert(7).unwrap();
    assert_ne!(fresh, cancelled);
    assert_eq!(slab.get(fresh), Some(&7));
}
