pub mod common;

use std::{cell, mem, num};

use common::{
    alloc::system::Tracker,
    depot::model::drops::{self, CALLS, Seed},
};
use o3::{
    alloc,
    depot::{
        arena::slab,
        capacity,
        queue::round::{self, Set, set, values},
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn keeps_hot_allocation() {
    let capacity = capacity::Count::new(64);
    let baseline = {
        let (slab, baseline) = Tracker::measure(|| slab::Store::<u64>::try_new(capacity));
        assert!(slab.is_ok(), "required fixture outcome");
        baseline
    };
    let (created, cold) = Tracker::measure(|| Set::<u64>::try_new(capacity));
    assert_eq!(cold, [baseline[0] + 1, baseline[1] + 64 * 8, 0, 0]);
    let mut owner = created.expect("allocate joint owner");
    let ((), hot) = Tracker::measure(|| {
        for (index, entry) in owner.entries().enumerate() {
            let entry = match entry {
                set::Entry::Vacant(entry) => Some(entry),
                _ => None,
            }
            .expect("fresh resident");
            let ready = entry.insert(index as u64).insert();
            let ready = ready.remove().insert();
            ready.requeue();
        }
        for _ in 0..256 {
            let head = owner.turn().expect("scheduled resident");
            head.advance();
        }
        while let Some(head) = owner.turn() {
            head.remove().discard();
        }
    });
    assert_eq!(hot, [0; 4]);
    let ((), freed) = Tracker::measure(move || assert!(owner.is_empty()));
    assert_eq!(freed, [0, 0, cold[0], cold[1]]);
}

#[test]
fn empty_never_allocates() {
    let (created, cold) =
        Tracker::measure(|| Tracker::fail_after(0, || Set::<u64>::try_new(capacity::Count::EMPTY)));
    let mut owner = created.expect("empty owner requests no memory");
    assert_eq!(cold, [0; 4]);
    assert!(owner.turn().is_none());
    let ((), freed) = Tracker::measure(move || assert!(owner.is_empty()));
    assert_eq!(freed, [0; 4]);
}

#[test]
fn exhaustion_keeps_failure() {
    for capacity in [1, 64, 256] {
        for allowed in 0..2 {
            let (created, counts) = Tracker::measure(|| {
                Tracker::fail_after(allowed, || {
                    Set::<u64>::try_new(capacity::Count::new(capacity))
                })
            });
            assert!(matches!(created, Err(alloc::Error::Exhausted)));
            assert_eq!(counts[0], counts[2]);
            assert_eq!(counts[1], counts[3]);
        }
    }
}

#[test]
fn keeps_joint_layout() {
    assert_eq!(mem::size_of::<Set<u64>>(), 48);
    assert_eq!(mem::size_of::<Option<Set<u64>>>(), 48);
    assert_eq!(mem::size_of::<set::Occupied<'static, u64>>(), 32);
    assert_eq!(
        mem::size_of::<set::Occupied<'static, u64, (), { u32::MAX }, true, (), set::Idle>>(),
        32
    );
    assert_eq!(
        mem::size_of::<set::Occupied<'static, u64, (), { u32::MAX }, true, (), set::Ready>>(),
        32
    );
    assert_eq!(
        mem::size_of::<set::Occupied<'static, u64, (), { u32::MAX }, true, (), set::Head>>(),
        32
    );
    assert_eq!(
        mem::size_of::<Option<set::Occupied<'static, u64, (), { u32::MAX }, true, (), set::Head>>>(
        ),
        32
    );
}

#[test]
fn lanes_keep_three_planes() {
    assert_eq!(mem::size_of::<round::Lanes<cell::Cell<usize>, 3>>(), 40);
    assert_eq!(
        mem::size_of::<Option<round::Lanes<cell::Cell<usize>, 3>>>(),
        40
    );
    for (capacity, lanes, bytes) in [(0, 3, 48), (1, 3, 72), (3, 2, 96), (5, 1, 120)] {
        let lanes = num::NonZero::<usize>::MIN.saturating_add(lanes - 1);
        let (result, cold) =
            Tracker::measure(|| round::Lanes::<cell::Cell<usize>, 3>::try_new(capacity, lanes));
        assert_eq!(cold, [1, bytes, 0, 0]);
        let queue = (result).expect("the three-plane fixture must allocate");
        let ((), hot) = Tracker::measure(|| {
            for lane in 0..lanes.get() {
                assert!(
                    round::Lane::try_new(&queue, lane)
                        .expect("lane")
                        .front()
                        .is_none()
                );
            }
            for index in 0..capacity {
                let lane = round::Lane::try_new(&queue, index % lanes.get()).expect("lane");
                let entry = queue.try_slot(index).expect("resident").get();
                assert_eq!(entry.get(), 0);
                entry.set(index);
                assert!(
                    queue
                        .try_slot(index)
                        .expect("resident")
                        .try_lane(index % lanes.get())
                        .expect("destination")
                        .set()
                        == round::Insertion::Inserted
                );
                assert_eq!(lane.pop_front().map(|slot| slot.get()), Some(index));
            }
        });
        assert_eq!(hot, [0; 4]);
        let ((), freed) = Tracker::measure(move || assert!(queue.group::<0>().active().is_none()));
        assert_eq!(freed, [0, 0, 1, bytes]);
    }
}

#[test]
fn aligned_lanes_drop_once() {
    CALLS.with(|calls| calls.set(Some(0)));
    let drops = cell::Cell::new(Some(0));
    let (result, cold) = Tracker::measure(|| {
        round::Lanes::<drops::Aligned<'_>, 3>::try_new(
            3,
            num::NonZero::<usize>::MIN.saturating_add(4),
        )
    });
    assert_eq!(cold, [1, 512, 0, 0]);
    let queue = (result).expect("the over-aligned borrowed values must allocate");
    let ((), hot) = Tracker::measure(|| {
        for index in 0..3 {
            let entry = queue.try_slot(index).expect("resident").get();
            assert_eq!(std::ptr::from_ref(entry).addr() % 64, 0);
            assert!(entry.0.0.get().is_none());
            entry.0.0.set(Some(&drops));
            assert!(
                queue
                    .try_slot(index)
                    .expect("resident")
                    .try_lane(index + 2)
                    .expect("destination")
                    .set()
                    == round::Insertion::Inserted
            );
        }
        assert_eq!(
            round::Lane::try_new(&queue, 3)
                .expect("lane")
                .pop_front()
                .map(|slot| std::ptr::eq(slot, queue.try_slot(1).expect("resident").get())),
            Some(true)
        );
        assert_eq!(drops.get(), Some(0));
    });
    assert_eq!(hot, [0; 4]);
    let ((), freed) =
        Tracker::measure(move || assert!(queue.try_slot(2).expect("resident").is_linked()));
    assert_eq!(freed, [0, 0, 1, 512]);
    assert_eq!(drops.get(), Some(3));
    assert_eq!(CALLS.with(cell::Cell::get), Some(0));
}

#[test]
fn lanes_reject_before_init() {
    CALLS.with(|calls| calls.set(Some(0)));

    for capacity in [0, 1, 257] {
        let (result, counts) = Tracker::measure(|| {
            Tracker::fail_after(0, || {
                round::Lanes::<Seed, 1>::try_new(capacity, num::NonZero::<usize>::MIN)
            })
        });
        assert!(matches!(result, Err(alloc::Error::Exhausted)));
        assert_eq!(counts, [0; 4]);
        assert_eq!(CALLS.with(cell::Cell::get), Some(0));
    }
    for (capacity, lanes) in [(usize::MAX, 1), (0, usize::MAX)] {
        let lanes = num::NonZero::<usize>::MIN.saturating_add(lanes - 1);
        let (result, counts) =
            Tracker::measure(|| round::Lanes::<Seed, 1>::try_new(capacity, lanes));
        assert!(matches!(result, Err(alloc::Error::Overflow)));
        assert_eq!(counts, [0; 4]);
        assert_eq!(CALLS.with(cell::Cell::get), Some(0));
    }
}

#[test]
fn reservations_are_hot() {
    let mut queue = (round::Lanes::<usize, 1>::try_new(1, num::NonZero::<usize>::MIN))
        .expect("reservation fixture must allocate");
    let ((), hot) = Tracker::measure(|| {
        assert!(
            queue
                .try_slot(0)
                .expect("resident")
                .try_lane(0)
                .expect("destination")
                .set()
                == round::Insertion::Inserted
        );
        let mut selected = round::Lane::try_new(&mut queue, 0).expect("lane");
        let mut held = (selected.dequeue()).expect("fresh resident must be reservable");
        assert_eq!(*held.get(), 0);
        for _ in 0..128 {
            held = held.push_back().remove();
        }
        assert_eq!(*held.get(), 0);
        assert!(
            queue
                .try_slot(0)
                .expect("resident")
                .try_lane(0)
                .expect("destination")
                .set()
                == round::Insertion::Inserted
        );
        let mut selected = round::Lane::try_new(&mut queue, 0).expect("lane");
        let held = (selected.dequeue()).expect("released resident must be reservable");
        assert_eq!(*held.get(), 0);
    });
    assert_eq!(hot, [0; 4]);
}

#[test]
fn values_keep_layout() {
    type Payload = num::NonZero<u64>;
    type Previous = round::Lanes<cell::Cell<Option<Payload>>, 2, 2>;
    let (baseline, cold) = Tracker::measure(|| Previous::try_fixed(3));
    drop(baseline.expect("previous layout"));
    let (created, counts) = Tracker::measure(|| values::Values::<Payload, 2, 2>::try_fixed(3));
    let queue = created.expect("value owner");
    assert_eq!(counts, [1, cold[1], 0, 0]);
    assert_eq!(mem::size_of_val(&queue), mem::size_of::<Previous>());
    let ((), hot) = Tracker::measure(|| {
        let first = queue.try_slot(0).expect("first resident");
        let second = queue.try_slot(1).expect("second resident");
        let third = queue.try_slot(2).expect("third resident");
        first.write(Payload::MIN);
        second.try_lane(0).expect("first lane").push(Payload::MIN);
        third.try_lane(1).expect("second lane").push(Payload::MIN);
        assert_eq!(queue.group::<0>().next(), Some(Payload::MIN));
        assert!(second.get().is_none());
        assert_eq!(first.get(), Some(Payload::MIN));
    });
    assert_eq!(hot, [0; 4]);
    let ((), freed) = Tracker::measure(move || drop(queue));
    assert_eq!(freed, [0, 0, 1, cold[1]]);
}
