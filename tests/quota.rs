use std::{mem, panic};

use o3::mem::quota::{Admission, Lease, Ledger, Permit, Shared};

enum Root {}
enum Child {}

#[test]
fn nested_reservations_refund_only_unused_quota() {
    let ledger = Ledger::<Root>::new(5);
    let mut shared = Shared::<Root>::reserve_exact(&ledger, 4).unwrap();
    assert_eq!(ledger.remaining(), 1);
    shared.spend(1);

    {
        let mut child = shared.lease_up_to::<Child>(2);
        assert!(child.take());
        assert_eq!(child.remaining(), 1);
    }

    assert_eq!(shared.remaining(), 2);
    drop(shared);
    assert_eq!(ledger.remaining(), 3);
}

#[test]
fn exact_up_to_and_all_reservations_preserve_the_ledger() {
    let ledger = Ledger::<Root>::new(3);
    assert!(Shared::<Root>::reserve_exact(&ledger, 4).is_none());
    assert_eq!(ledger.remaining(), 3);

    let mut lease = Lease::<Child>::reserve_up_to(&ledger, 8);
    assert_eq!(lease.remaining(), 3);
    lease.spend(2);
    drop(lease);
    assert_eq!(ledger.remaining(), 1);

    let lease = Lease::<Child>::reserve_all(&ledger);
    assert_eq!(lease.remaining(), 1);
    assert_eq!(ledger.remaining(), 0);
    drop(lease);
    assert_eq!(ledger.remaining(), 1);
}

#[test]
fn admission_charges_only_an_acquired_item() {
    let mut ledger = Ledger::<Root>::new(1);
    assert!(matches!(
        ledger.admit_with(|| None::<usize>),
        Admission::Empty
    ));
    assert_eq!(ledger.remaining(), 1);
    assert!(matches!(ledger.admit_with(|| Some(5)), Admission::Item(5)));

    let mut called = false;
    assert!(matches!(
        ledger.admit_with(|| {
            called = true;
            Some(6)
        }),
        Admission::Exhausted
    ));
    assert!(!called);

    ledger.reset(1);
    let mut lease = Lease::<Child>::reserve_all(&ledger);

    assert!(matches!(
        lease.admit_with(|| None::<usize>),
        Admission::Empty
    ));
    assert_eq!(lease.remaining(), 1);
    assert!(matches!(lease.admit_with(|| Some(7)), Admission::Item(7)));

    let mut called = false;
    assert!(matches!(
        lease.admit_with(|| {
            called = true;
            Some(9)
        }),
        Admission::Exhausted
    ));
    assert!(!called);
}

#[test]
fn unwinding_refunds_the_unspent_remainder() {
    let ledger = Ledger::<Root>::new(4);
    let unwind = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let mut lease = Lease::<Child>::reserve_all(&ledger);
        assert!(lease.take());
        panic!("stop after one unit");
    }));
    assert!(unwind.is_err());
    assert_eq!(ledger.remaining(), 3);
}

#[test]
fn permit_charges_once_and_retains_the_exclusive_borrow() {
    let ledger = Ledger::<Root>::new(2);
    let mut lease = Lease::<Child>::reserve_all(&ledger);
    let permit = lease.take_permit().expect("one quota unit");
    std::hint::black_box(permit);
    assert_eq!(lease.remaining(), 1);
    drop(lease);
    assert_eq!(ledger.remaining(), 1);
}

#[test]
fn quota_types_keep_the_original_scheduler_layouts() {
    assert_eq!(mem::size_of::<Ledger<Root>>(), mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<Shared<'static, Root>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Lease<'static, Child>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(mem::size_of::<Permit<'static>>(), 0);
}
