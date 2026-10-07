use std::{mem, num::NonZero};

use o3::mem::credit::Ledger;

#[test]
fn tracks_credit() {
    const THREE: NonZero<usize> = NonZero::<usize>::MIN.saturating_add(2);
    const FIVE: NonZero<usize> = NonZero::<usize>::MIN.saturating_add(4);

    let ledger = Ledger::new(5);
    let other = Ledger::new(5);
    assert_eq!(ledger.limit(), 5);
    assert_eq!(ledger.available(), 5);
    let charge = ledger.try_acquire(THREE).expect("three credits fit");
    let reserved = other.try_acquire(FIVE).expect("all other credits fit");
    assert_eq!(*charge.item(), THREE);
    assert_eq!(ledger.available(), 2);
    let Err(error) = ledger.try_acquire(THREE) else {
        panic!("only two remain");
    };
    assert_eq!(error.into_parts(), (THREE, 2));
    assert_eq!(ledger.available(), 2);
    drop(charge);
    assert_eq!((ledger.available(), other.available()), (5, 0));
    drop(reserved);
    assert_eq!((ledger.available(), other.available()), (5, 5));
}

#[test]
fn returns_full_width() {
    let ledger = Ledger::new(usize::MAX);
    let amount = NonZero::new(usize::MAX - 1).expect("positive first portion");
    let first = ledger.try_acquire(amount).expect("first portion fits");
    let last = ledger
        .try_acquire(NonZero::<usize>::MIN)
        .expect("last unit fits");
    let Err(error) = ledger.try_acquire(NonZero::<usize>::MIN) else {
        panic!("full ledger");
    };
    assert_eq!(error.into_parts(), (NonZero::<usize>::MIN, 0));
    drop(first);
    assert_eq!(ledger.available(), usize::MAX - 1);
    drop(last);
    assert_eq!(ledger.available(), usize::MAX);
    mem::forget(
        ledger
            .try_acquire(NonZero::<usize>::MIN)
            .expect("one unit fits"),
    );
    drop(ledger.try_acquire(amount).expect("unforgotten credit fits"));
    assert_eq!(ledger.available(), usize::MAX - 1);
}

#[test]
fn rejects_empty_ledger() {
    let ledger = Ledger::new(0);
    let Err(error) = ledger.try_acquire(NonZero::<usize>::MAX) else {
        panic!("no credit");
    };
    assert_eq!(error.into_parts(), (NonZero::<usize>::MAX, 0));
    assert_eq!(ledger.available(), 0);
}
