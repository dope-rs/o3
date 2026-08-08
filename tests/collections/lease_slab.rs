use std::{
    cell::Cell,
    mem::{forget, size_of},
    panic::{AssertUnwindSafe, catch_unwind},
};

use o3::collections::slab::{Capacity, lease};

use crate::support::PanicDrop;

fn panic_value() -> u8 {
    panic!("construction panic");
}

#[test]
fn leases_are_one_word_and_borrow_the_slab() {
    assert_eq!(size_of::<lease::Lease<'static, u64>>(), size_of::<usize>());

    let slab = lease::Pool::with_capacity(Capacity::new(1));
    let mut lease = slab.vacant_entry().expect("vacant slot").insert(7u64);
    assert_eq!(*lease, 7);
    *lease = 9;
    assert_eq!(*lease, 9);
}

#[test]
fn cancelled_vacancies_and_dropped_leases_recycle_slots() {
    let slab = lease::Pool::with_capacity(Capacity::new(1));
    {
        let _entry = slab.vacant_entry().expect("vacant slot");
    }
    assert!(slab.vacant_entry().is_some());

    let first = slab
        .vacant_entry()
        .expect("first lease")
        .insert(String::from("first"));
    assert!(slab.vacant_entry().is_none());
    drop(first);
    let second = slab
        .vacant_entry()
        .expect("recycled lease")
        .insert(String::from("second"));
    assert_eq!(&*second, "second");
}

#[test]
fn panicking_value_drop_still_reclaims_the_slot() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let slab = lease::Pool::with_capacity(Capacity::new(1));
    let lease = slab
        .vacant_entry()
        .expect("panic lease")
        .insert(PanicDrop::new(0, &drops, &panic_once));

    let caught = catch_unwind(AssertUnwindSafe(|| drop(lease)));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 1);
    let replacement = slab.vacant_entry().expect("reclaimed slot");
    drop(replacement);

    let replacement = slab
        .vacant_entry()
        .expect("replacement lease")
        .insert(PanicDrop::new(1, &drops, &panic_once));
    drop(replacement);
    assert_eq!(drops.get(), 2);
}

#[test]
fn zero_capacity_is_permanently_full() {
    let slab = lease::Pool::<u8>::with_capacity(Capacity::EMPTY);
    assert!(slab.vacant_entry().is_none());
}

#[test]
fn panicking_construction_cancels_the_vacancy() {
    let slab = lease::Pool::<u8>::with_capacity(Capacity::new(1));
    let caught = catch_unwind(AssertUnwindSafe(|| {
        let entry = slab.vacant_entry().expect("vacant slot");
        entry.insert(panic_value());
    }));
    assert!(caught.is_err());
    assert!(slab.vacant_entry().is_some());
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
        let slab = lease::Pool::with_capacity(Capacity::new(1));
        let lease = slab
            .vacant_entry()
            .expect("vacant slot")
            .insert(CountDrop(&drops));
        forget(lease);
        assert!(slab.vacant_entry().is_none());
    }
    assert_eq!(drops.get(), 1);
}
