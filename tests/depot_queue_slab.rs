pub mod common;

use std::{cell::Cell, mem};

use common::{
    Counted,
    depot::queue::indexed::slab::{Tag, layout::Aligned},
};
use o3::depot::{capacity, queue::indexed::slab};

#[test]
fn rejects_stale_handles() {
    let mut fifo = (slab::Slab::<usize, Tag>::try_from(capacity::Count::new(2)))
        .expect("the two-slot stale-handle FIFO must allocate");
    let stale = (fifo.vacant().map(|vacant| vacant.insert(10).key()))
        .expect("the first indexed value must fit");
    let second = (fifo.vacant().map(|vacant| vacant.insert(20).key()))
        .expect("the second indexed value must fit");

    assert_eq!(fifo.entry(stale).expect("live fifo entry").remove(), 10);
    let fresh = (fifo.vacant().map(|vacant| vacant.insert(30).key()))
        .expect("the removed indexed slot must accept a recycled value");
    assert_eq!(fresh.index(), stale.index());
    assert!(fresh != stale);
    assert!(fifo.entry(stale).is_none());
    assert!(fifo.front_pair().map(|pair| pair.0) == Some(second));
}

#[test]
fn rejects_failed_handles() {
    let mut fifo = (slab::Slab::<usize, Tag>::try_from(capacity::Count::new(1)))
        .expect("the one-slot FIFO must allocate");
    let vacant = (fifo.vacant()).expect("the empty FIFO must have a vacancy");
    let stale = vacant
        .try_insert(Err::<usize, _>)
        .err()
        .expect("rejected key");
    assert!(fifo.is_empty());
    assert_eq!(fifo.available(), 1);
    let fresh = (fifo.vacant().map(|vacant| vacant.insert(11).key()))
        .expect("the rejected insertion must release capacity");
    assert_eq!(fresh.index(), stale.index());
    assert!(fresh != stale);
    assert!(fifo.entry(stale).is_none());
    assert!(fifo.front_pair().map(|pair| pair.0) == Some(fresh));
}

#[test]
fn retires_failed_epoch() {
    let mut fifo = (slab::Slab::<usize, Tag, 1>::try_from(capacity::Count::new(1)))
        .expect("the one-epoch FIFO must allocate");
    let vacant = (fifo.vacant()).expect("the empty FIFO must have its first epoch");
    let stale = vacant
        .try_insert(Err::<usize, _>)
        .err()
        .expect("rejected key");
    assert!(fifo.entry(stale).is_none());
    assert!(fifo.is_empty());
    assert_eq!(fifo.available(), 0);
    assert!(fifo.vacant().is_none());
}

#[test]
fn keeps_compact_layout() {
    assert_eq!(mem::size_of::<slab::Handle<Tag>>(), 8);
    assert_eq!(mem::size_of::<Option<slab::Handle<Tag>>>(), 8);
    assert_eq!(mem::size_of::<slab::Slab<u64, Tag>>(), 32);
}

#[test]
fn aligns_and_drops() {
    let drops = Cell::new(Some(0));
    {
        let mut fifo = (slab::Slab::<Aligned<'_>, Tag>::try_from(capacity::Count::new(2)))
            .expect("the two-slot over-aligned FIFO must allocate");
        assert!(
            fifo.vacant()
                .map(|vacant| vacant.insert(Aligned(Counted(&drops))))
                .is_some()
        );
        assert!(
            fifo.vacant()
                .map(|vacant| vacant.insert(Aligned(Counted(&drops))))
                .is_some()
        );

        let addr = fifo.front_pair().expect("aligned fifo front").1 as *const Aligned<'_> as usize;
        assert_eq!(addr % mem::align_of::<Aligned<'_>>(), 0);
        {
            let popped = fifo.pop_front();
            assert!(popped.is_some());
        }
        assert_eq!(drops.get(), Some(1));
    }
    assert_eq!(drops.get(), Some(2));
}
