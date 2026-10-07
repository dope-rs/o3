pub mod common;

use std::{cell::Cell, num::NonZero};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::model::{Tag, drops::Tracked},
};
use o3::depot::{
    arena::slab::Store,
    capacity::{self, Count},
    queue::{
        indexed::{fifo, slab},
        pool,
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn seeded_fifo_keeps_owner() {
    let drops = Cell::new(Some(0));
    let rejected = Tracker::fail_after(0, || {
        fifo::List::try_with(NonZero::<u32>::MIN, (7, Counted(&drops)))
    });
    assert!(matches!(rejected, Err(o3::alloc::Error::Exhausted)));
    assert_eq!(drops.get(), Some(1));
    let mut queue =
        fifo::List::try_with(NonZero::<u32>::MIN, (11, Counted(&drops))).expect("one seeded slot");
    assert_eq!(
        (queue.capacity(), queue.len(), drops.get()),
        (1, 1, Some(1))
    );
    assert!(matches!(queue.entry(0), Some(fifo::Entry::Occupied(entry)) if entry.get().0 == 11));
    drop(queue);
    assert_eq!(drops.get(), Some(2));
}

fn assert_each_drop<'a>(
    drops: &'a [Cell<Option<usize>>; 2],
    run: impl FnOnce(Tracked<'a>, Tracked<'a>),
) {
    for drops in drops {
        drops.set(Some(0));
    }
    run(
        Tracked::try_new(0, drops).expect("drop counter exists"),
        Tracked::try_new(1, drops).expect("drop counter exists"),
    );
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn drops_owned_values_once() {
    let drops = [Cell::new(Some(0)), Cell::new(Some(0))];

    assert_each_drop(&drops, |first, second| {
        let mut slab = (Store::<Tracked<'_>>::try_new(Count::new(2)))
            .expect("the two-slot owned-value slab must allocate");
        assert!(slab.insert(first).is_ok(), "required fixture outcome");
        assert!(slab.insert(second).is_ok(), "required fixture outcome");
    });
    assert_each_drop(&drops, |first, second| {
        let mut queue = (fifo::List::<Tracked<'_>>::try_new(2))
            .expect("the two-slot owned indexed FIFO must allocate");
        let slot = (queue.vacant_entry(0)).expect("index zero must be vacant");
        slot.push_back(first);
        let slot = (queue.vacant_entry(1)).expect("index one must be vacant");
        slot.push_back(second);
    });
    assert_each_drop(&drops, |first, second| {
        let mut queue = (slab::Slab::<Tracked<'_>, Tag>::try_from(capacity::Count::new(2)))
            .expect("the two-slot owned generational FIFO must allocate");
        assert!(queue.vacant().map(|vacant| vacant.insert(first)).is_some());
        assert!(queue.vacant().map(|vacant| vacant.insert(second)).is_some());
    });
    assert_each_drop(&drops, |first, second| {
        let plan = (pool::Plan::new(2, 1)).expect("one two-slot queue lane is a valid pool plan");
        let mut arena = (pool::Lanes::<Tracked<'_>>::try_new(plan))
            .expect("the one-lane queue pool must allocate");
        let mut lane = (arena.get_mut(0)).expect("lane zero must exist");
        assert!(lane.push_back(first).is_ok(), "required fixture outcome");
        assert!(lane.push_back(second).is_ok(), "required fixture outcome");
    });
}

#[test]
fn clear_restores_slab() {
    let drops = [Cell::new(Some(0)), Cell::new(Some(0)), Cell::new(Some(0))];
    {
        let mut slab = (Store::<Tracked<'_>>::try_new(Count::new(2)))
            .expect("the two-slot clearable slab must allocate");
        assert!(
            slab.insert(Tracked::try_new(0, &drops).expect("drop counter exists"))
                .is_ok(),
            "required fixture outcome"
        );
        assert!(
            slab.insert(Tracked::try_new(1, &drops).expect("drop counter exists"))
                .is_ok(),
            "required fixture outcome"
        );
        slab.clear();
        assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY);
        assert!(!slab.is_full());
        assert_eq!(drops.each_ref().map(Cell::get), [1, 1, 0].map(Some));
        assert!(
            slab.insert(Tracked::try_new(2, &drops).expect("drop counter exists"))
                .is_ok(),
            "required fixture outcome"
        );
        assert_eq!(drops.each_ref().map(Cell::get), [1, 1, 0].map(Some));
    }
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 3]);
}
