pub mod common;

use std::{cell::Cell, mem};

use common::{
    Counted,
    alloc::{AlignedUnit, system::Tracker},
};
use o3::depot::queue::resident::{Drain, Fixed, Take, handoff};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn retains_affine_grants() {
    let drops = [Cell::new(Some(0)), Cell::new(Some(0)), Cell::new(Some(0))];
    let registry = handoff::Registry::try_new(1, Fixed::<2>).expect("registry");
    let other = handoff::Registry::try_new(1, Fixed::<2>).expect("other registry");
    let empty = handoff::Registry::try_new(0, Fixed::<2>).expect("empty registry");
    let mut slot = handoff::Registration::new();
    registry
        .lane::<0>()
        .try_keep(&mut slot, 7)
        .expect("register");
    registry.lane::<1>().try_keep(&mut slot, 9).expect("relink");
    assert!(registry.lane::<0>().is_empty());
    assert!(matches!(slot.take_grant(), handoff::Grant::Pending));
    assert_eq!(
        empty.lane::<0>().try_keep(&mut slot, 1),
        Err(handoff::Reject::Active { index: 0 })
    );
    assert_eq!(
        empty
            .lane::<0>()
            .try_keep(&mut handoff::Registration::new(), 1),
        Err(handoff::Reject::Full { index: 0 })
    );
    assert!(matches!(
        registry.lane::<1>().offer_one(Counted(&drops[0])),
        handoff::Offer::Value(9)
    ));
    assert_eq!(
        other.lane::<0>().try_keep(&mut slot, 11),
        Err(handoff::Reject::GrantPending { index: 0 })
    );
    assert_eq!(
        empty.lane::<0>().try_keep(&mut slot, 11),
        Err(handoff::Reject::GrantPending { index: 0 })
    );
    let grant = match slot.take_grant() {
        handoff::Grant::Ready(grant) => Some(grant),
        handoff::Grant::Empty | handoff::Grant::Pending => None,
    }
    .expect("grant");
    assert!(matches!(slot.take_grant(), handoff::Grant::Empty));
    assert_eq!(drops[0].get(), Some(0));
    drop(grant);
    assert_eq!(drops[0].get(), Some(1));
    other
        .lane::<0>()
        .try_keep(&mut slot, 12)
        .expect("move after grant");
    assert!(other.lane::<0>().take_one() == Take::Item(12));
    other.lane::<0>().try_keep(&mut slot, 13).expect("rearm");
    assert!(matches!(slot.cancel(), handoff::Cancel::Wake));
    assert!(other.lane::<0>().is_empty());
    assert!(matches!(slot.cancel(), handoff::Cancel::Empty));

    registry.lane::<0>().try_keep(&mut slot, 13).expect("reuse");
    assert!(matches!(
        registry.lane::<0>().offer_one(Counted(&drops[1])),
        handoff::Offer::Value(13)
    ));
    let grant = match slot.cancel() {
        handoff::Cancel::Grant(grant) => Some(grant),
        handoff::Cancel::Empty | handoff::Cancel::Wake => None,
    }
    .expect("cancel");
    assert!(matches!(slot.cancel(), handoff::Cancel::Empty));
    assert_eq!(drops[1].get(), Some(0));
    drop(grant);
    assert_eq!(drops[1].get(), Some(1));

    registry.lane::<1>().try_keep(&mut slot, 17).expect("reuse");
    assert!(matches!(
        registry.lane::<1>().offer_one(Counted(&drops[2])),
        handoff::Offer::Value(17)
    ));
    drop(slot);
    assert_eq!(drops[2].get(), Some(1));
    let mut next = handoff::Registration::new();
    registry
        .lane::<0>()
        .try_keep(&mut next, 19)
        .expect("returned slot");
    assert_eq!(
        registry
            .lane::<0>()
            .try_keep(&mut handoff::Registration::new(), 23),
        Err(handoff::Reject::Full { index: 0 })
    );
}

#[test]
fn drains_and_reuses() {
    let (result, cold) =
        Tracker::counts(|| handoff::Registry::<usize, usize, _>::try_new(3, Fixed::<3>));
    assert_eq!(cold, [1, 0]);
    let registry = result.expect("registry");
    let ((), hot) = Tracker::measure(|| {
        let mut first = handoff::Registration::new();
        let mut second = handoff::Registration::new();
        let mut third = handoff::Registration::new();
        registry
            .lane::<0>()
            .try_keep(&mut first, 10)
            .expect("first handoff");
        registry
            .lane::<0>()
            .try_keep(&mut second, 20)
            .expect("second handoff");
        registry
            .lane::<1>()
            .try_keep(&mut first, 11)
            .expect("move first");
        assert!(registry.lane::<0>().offer_one(2) == handoff::Offer::Value(20));
        assert!(second.take_grant() == handoff::Grant::Ready(2));
        assert!(registry.lane::<1>().offer_one(1) == handoff::Offer::Value(11));
        assert!(first.take_grant() == handoff::Grant::Ready(1));
        registry.lane::<2>().try_keep(&mut first, 1).expect("first");
        registry
            .lane::<1>()
            .try_keep(&mut second, 2)
            .expect("second");
        assert!(registry.has_waiters());
        registry.begin_drain();
        assert!(registry.is_draining());
        assert_eq!(
            registry.lane::<0>().try_keep(&mut third, 0),
            Err(handoff::Reject::Draining { index: 0 })
        );
        assert!(registry.lane::<2>().offer_one(4) == handoff::Offer::Draining(4));
        assert!(registry.drain_next() == Drain::Item(1));
        assert!(registry.has_waiters());
        assert!(registry.drain_next() == Drain::Item(2));
        assert!(!registry.has_waiters());
        assert!(registry.drain_next() == Drain::Done);
        assert!(!registry.is_draining());
        registry
            .lane::<0>()
            .try_keep(&mut third, 3)
            .expect("drain ended");
        assert!(registry.lane::<0>().offer_one(5) == handoff::Offer::Value(3));
        assert!(third.take_grant() == handoff::Grant::Ready(5));
        registry.lane::<0>().try_keep(&mut third, 6).expect("reuse");
    });
    assert_eq!(hot, [0; 4]);
    assert!(!registry.has_waiters());
    registry
        .lane::<0>()
        .try_keep(&mut handoff::Registration::new(), 7)
        .expect("returned slot");
    let ((), freed) = Tracker::counts(move || assert!(!registry.has_waiters()));
    assert_eq!(freed, [0, 1]);
}

#[test]
fn storage_keeps_alignment() {
    let word = mem::size_of::<usize>();
    assert_eq!(
        mem::size_of::<handoff::Registry<usize, usize, Fixed<2>>>(),
        5 * word
    );
    assert_eq!(
        mem::size_of::<Option<handoff::Registration<'static, usize, usize, Fixed<2>>>>(),
        3 * word
    );
    let drops = Cell::new(Some(0));
    let (result, cold) = Tracker::measure(|| {
        handoff::Registry::<usize, (AlignedUnit, Counted<&Cell<Option<usize>>>), _>::try_new(
            3, Fixed::<5>,
        )
    });
    assert_eq!(cold, [1, 512, 0, 0]);
    let registry = result.expect("aligned grant storage");
    let ((), hot) = Tracker::measure(|| {
        let mut slot = handoff::Registration::new();
        registry
            .lane::<4>()
            .try_keep(&mut slot, 7)
            .expect("aligned slot");
        assert!(matches!(
            registry
                .lane::<4>()
                .offer_one((AlignedUnit, Counted(&drops))),
            handoff::Offer::Value(7)
        ));
        assert_eq!(drops.get(), Some(0));
    });
    assert_eq!(hot, [0; 4]);
    assert_eq!(drops.get(), Some(1));
    let ((), freed) = Tracker::measure(move || assert!(!registry.has_waiters()));
    assert_eq!(freed, [0, 0, 1, 512]);
    assert_eq!(drops.get(), Some(1));
}
