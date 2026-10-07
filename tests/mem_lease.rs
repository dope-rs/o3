pub mod common;

use std::cell::Cell;

use common::{
    Counted,
    mem::{RefSink, Sink},
};
use o3::mem::{self, Lease, lease};

#[test]
fn rejection_keeps_owner() {
    let drops = Cell::new(Some(0));
    {
        let rejected = mem::Rejected::new(Counted(&drops), 7u8).map(Some);
        assert_eq!(drops.get(), Some(0));
        let (owner, error) = rejected.into_parts();
        assert_eq!(error, 7);
        assert_eq!(drops.get(), Some(0));
        assert!(owner.is_some());
    }
    assert_eq!(drops.get(), Some(1));
}

#[test]
fn rejection_discards_once() {
    let drops = Cell::new(Some(0));
    let rejected = mem::Rejected::new(Counted(&drops), 9u8);
    assert_eq!(rejected.discard(), 9);
    assert_eq!(drops.get(), Some(1));
    assert_eq!(
        std::mem::size_of::<mem::Rejected<&(), usize>>(),
        2 * std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<Option<mem::Rejected<&(), usize>>>(),
        std::mem::size_of::<mem::Rejected<&(), usize>>()
    );
}

#[test]
fn returns_or_disarms() {
    let slot = Cell::new(None);
    let calls = Cell::new(Some(0));
    let lease = Lease::open(
        Sink {
            returned: &slot,
            calls: &calls,
        },
        7,
    );
    assert!(std::ptr::eq(lease.sink().returned, &slot));
    assert_eq!(*lease.item(), 7);
    assert_eq!(
        std::mem::size_of_val(&lease),
        std::mem::size_of::<(Sink<'_>, u8)>()
    );
    let (sink, item) = lease.into_parts();
    assert!(std::ptr::eq(sink.returned, &slot));
    assert_eq!(item, 7);
    assert_eq!(slot.get(), None);
    assert_eq!(calls.get(), Some(0));
    {
        let lease = Lease::new(sink, item);
        assert_eq!(lease.item(), &7);
    }
    assert_eq!(slot.get(), Some(7));
    assert_eq!(calls.get(), Some(1));
}

#[test]
fn keeps_reference_niche() {
    let returns = Cell::new(Some(0));
    assert_eq!(Lease::new(RefSink(&returns), ()).item(), &());
    assert_eq!(returns.get(), Some(1));
    assert_eq!(
        std::mem::size_of::<Lease<RefSink<'_>>>(),
        std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<Option<Lease<RefSink<'_>>>>(),
        std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<Lease<RefSink<'_>, lease::Open>>(),
        std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<Option<Lease<RefSink<'_>, lease::Open>>>(),
        std::mem::size_of::<usize>()
    );
}
