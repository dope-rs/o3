pub mod common;

use common::mem::{Child, Root};
use o3::mem::quota::{self, Admission, Lease, Ledger, Shared};

#[test]
fn refunds_unused_quota() {
    let ledger = Ledger::<Root>::new(5);
    {
        let shared = Shared::<Root>::reserve(&ledger, 4).expect("ledger begins with five units");
        assert_eq!(ledger.remaining(), 1);

        {
            let mut child = shared.lease_up_to::<Child>(2);
            assert!(matches!(child.admit_with(|| Some(())), Admission::Item(())));
            assert_eq!(child.remaining(), 1);
        }

        assert_eq!(shared.remaining(), 3);
    }
    assert_eq!(ledger.remaining(), 4);
}

#[test]
fn preserves_reservations() {
    let ledger = Ledger::<Root>::new(3);
    assert!(Shared::<Root>::reserve(&ledger, 4).is_none());
    assert_eq!(ledger.remaining(), 3);

    {
        let mut lease = Lease::<Child>::reserve_up(&ledger, 8);
        assert_eq!(lease.remaining(), 3);
        assert!(matches!(lease.admit_with(|| Some(())), Admission::Item(())));
        assert!(matches!(lease.admit_with(|| Some(())), Admission::Item(())));
    }
    assert_eq!(ledger.remaining(), 1);

    {
        let lease = Lease::<Child>::reserve_all(&ledger);
        assert_eq!(lease.remaining(), 1);
        assert_eq!(ledger.remaining(), 0);
    }
    assert_eq!(ledger.remaining(), 1);
}

#[test]
fn charges_acquired_item() {
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
fn refunds_or_consumes_open_unit() {
    let ledger = Ledger::<Root>::new(1);
    {
        let lease = ledger.open();
        assert!(lease.is_some());
        assert_eq!(ledger.remaining(), 0);
        assert!(ledger.open().is_none());
    }
    assert_eq!(ledger.remaining(), 1);
    let lease = ledger.open().expect("refunded unit can be readmitted");
    assert_eq!(ledger.remaining(), 0);
    let (_, ()) = lease.into_parts();
    assert_eq!(ledger.remaining(), 0);
}

#[test]
fn debits_before_reentry() {
    let ledger = Ledger::<Root>::new(2);
    assert!(matches!(
        ledger.admit_with(|| {
            assert_eq!(ledger.remaining(), 1);
            ledger.apply(|| {
                assert_eq!(ledger.remaining(), 0);
                assert!(matches!(ledger.take(), quota::Take::Exhausted));
                quota::Acquire::Empty
            });
            assert_eq!(ledger.remaining(), 1);
            assert!(matches!(ledger.take(), quota::Take::Taken));
            None::<()>
        }),
        Admission::Empty
    ));
    assert_eq!(ledger.remaining(), 1);
    ledger.apply(|| {
        assert_eq!(ledger.remaining(), 0);
        quota::Acquire::Item
    });
    assert_eq!(ledger.remaining(), 0);
    let mut called = false;
    ledger.apply(|| {
        called = true;
        quota::Acquire::Empty
    });
    assert!(!called);
}

#[test]
fn returns_full_width() {
    let source = Ledger::<Root>::new(usize::MAX);
    let other = Ledger::<Root>::new(7);
    {
        let empty = Shared::<Child>::reserve(&source, 0).expect("zero quota fits");
        assert_eq!(empty.remaining(), 0);
        assert_eq!(source.remaining(), usize::MAX);
    }
    {
        let shared = Shared::<Child>::reserve(&source, usize::MAX).expect("full width fits");
        assert_eq!(source.remaining(), 0);
        {
            let tail = shared.lease_up_to::<Child>(1);
            assert_eq!(tail.remaining(), 1);
            {
                let first = shared.lease_up_to::<Child>(usize::MAX);
                assert_eq!(first.remaining(), usize::MAX - 1);
                assert_eq!(shared.remaining(), 0);
            }
            assert_eq!(shared.remaining(), usize::MAX - 1);
        }
        assert_eq!(shared.remaining(), usize::MAX);
    }
    assert_eq!(source.remaining(), usize::MAX);
    assert_eq!(other.remaining(), 7);
}
