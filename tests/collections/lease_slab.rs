use std::cell::Cell;
use std::mem::{forget, size_of, size_of_val};
use std::panic::{AssertUnwindSafe, catch_unwind};

use o3::collections::{LeaseSlab, SlabLease};

use crate::support::PanicDrop;

fn panic_value() -> u8 {
    panic!("construction panic");
}

#[test]
fn leases_are_one_word_and_borrow_the_slab() {
    assert_eq!(size_of::<SlabLease<'static, u64>>(), size_of::<usize>());

    let slab = LeaseSlab::try_with_capacity(1).expect("lease slab");
    assert_eq!(size_of_val(&slab), size_of::<usize>());
    let mut lease = slab.insert(7u64).expect("vacant slot");
    assert_eq!(slab.len(), 1);
    assert!(!slab.is_empty());
    assert_eq!(slab.available(), 0);
    assert_eq!(*lease, 7);
    *lease = 9;
    assert_eq!(*lease, 9);
}

#[test]
fn cancelled_vacancies_and_dropped_leases_recycle_slots() {
    let slab = LeaseSlab::try_with_capacity(1).expect("lease slab");
    {
        let entry = slab.vacant_entry().expect("vacant slot");
        assert_eq!(entry.index(), 0);
        assert!(slab.is_full());
    }
    assert_eq!(slab.available(), 1);

    let first = slab.insert(String::from("first")).expect("first lease");
    assert!(slab.insert(String::from("full")).is_err());
    drop(first);
    let second = slab.insert(String::from("second")).expect("recycled lease");
    assert_eq!(&*second, "second");
}

#[test]
fn panicking_value_drop_still_reclaims_the_slot() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let slab = LeaseSlab::try_with_capacity(1).expect("lease slab");
    let lease = slab
        .insert(PanicDrop::new(0, &drops, &panic_once))
        .ok()
        .expect("panic lease");

    let caught = catch_unwind(AssertUnwindSafe(|| drop(lease)));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 1);
    assert_eq!(slab.available(), 1);

    let replacement = slab
        .insert(PanicDrop::new(1, &drops, &panic_once))
        .ok()
        .expect("replacement lease");
    drop(replacement);
    assert_eq!(drops.get(), 2);
}

#[test]
fn zero_capacity_is_permanently_full() {
    let slab = LeaseSlab::<u8>::try_with_capacity(0).expect("empty lease slab");
    assert_eq!(slab.capacity(), 0);
    assert_eq!(slab.len(), 0);
    assert!(slab.is_empty());
    assert_eq!(slab.available(), 0);
    assert!(slab.is_full());
    assert!(slab.insert(1).is_err());
}

#[test]
fn panicking_construction_cancels_the_vacancy() {
    let slab = LeaseSlab::<u8>::try_with_capacity(1).expect("lease slab");
    let caught = catch_unwind(AssertUnwindSafe(|| {
        let entry = slab.vacant_entry().expect("vacant slot");
        entry.insert(panic_value());
    }));
    assert!(caught.is_err());
    assert_eq!(slab.available(), 1);
}

#[test]
fn forgotten_lease_value_is_dropped_with_the_slab() {
    struct CountDrop<'a>(&'a Cell<usize>);

    impl Drop for CountDrop<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let drops = Cell::new(0);
    {
        let slab = LeaseSlab::try_with_capacity(1).expect("lease slab");
        let lease = slab.insert(CountDrop(&drops)).ok().expect("vacant slot");
        forget(lease);
        assert_eq!(slab.len(), 1);
    }
    assert_eq!(drops.get(), 1);
}
