pub mod common;

use std::{cell, num};

use common::{alloc::system::Tracker, depot::model::drops};
use o3::depot::queue::round::{self, values};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn rejects_bounds() {
    let mut queue =
        round::Lanes::<u64, 2>::try_new(2, num::NonZero::<usize>::MIN.saturating_add(1))
            .expect("queue allocation");
    let ((), counts) = Tracker::measure(|| {
        for index in [2, usize::MAX] {
            let error = queue.try_slot(index).err().expect("resident bounds");
            assert_eq!(error, round::Bounds::Slot { index, capacity: 2 });
        }
        for index in [2, usize::MAX] {
            let error = round::Lane::try_new(&queue, index)
                .err()
                .expect("shared lane bounds");
            assert_eq!(error, round::Bounds::Lane { index, capacity: 2 });
            let error = round::Lane::try_new(&mut queue, index)
                .err()
                .expect("exclusive lane bounds");
            assert_eq!(error, round::Bounds::Lane { index, capacity: 2 });
            let slot = queue.try_slot(0).expect("resident");
            let error = slot.try_lane(index).expect_err("destination bounds");
            assert_eq!(error, round::Bounds::Lane { index, capacity: 2 });
        }
        assert!(queue.group::<0>().active().is_none());
        assert!(queue.group::<1>().active().is_none());
        queue
            .try_slot(1)
            .expect("resident")
            .try_lane(1)
            .expect("lane")
            .push();
        assert_eq!(
            queue
                .group::<1>()
                .next()
                .map(|(lane, value)| (lane, *value)),
            Some((1, 0))
        );
        assert!(queue.group::<1>().active().is_none());
    });
    assert_eq!(counts, [0; 4]);
    let empty = round::Lanes::<u64, 1>::try_new(0, num::NonZero::<usize>::MIN)
        .expect("empty domain allocation");
    let error = empty
        .try_slot(0)
        .err()
        .expect("empty domain has no resident");
    assert_eq!(
        error,
        round::Bounds::Slot {
            index: 0,
            capacity: 0
        }
    );
}

#[test]
fn stripes_drop_once() {
    drops::CALLS.with(|calls| calls.set(Some(0)));
    drop(drops::Deferred::default());
    drops::CALLS.with(|calls| assert_eq!(calls.replace(Some(0)), Some(1)));
    let drops = cell::Cell::new(Some(0));
    {
        let queue = round::Lanes::<drops::Deferred<'_>, 3>::try_new(
            3,
            num::NonZero::<usize>::MIN.saturating_add(5),
        );
        let queue = queue.expect("required fixture outcome");
        for index in 0..3 {
            let entry = queue.try_slot(index).expect("resident").get();
            assert!(entry.0.get().is_none());
            entry.0.set(Some(&drops));
        }
        assert!(
            queue
                .try_slot(0)
                .expect("resident")
                .try_lane(0)
                .expect("destination")
                .set()
                == round::Insertion::Inserted
        );
        assert!(
            queue
                .try_slot(1)
                .expect("resident")
                .try_lane(3)
                .expect("destination")
                .set()
                == round::Insertion::Inserted
        );
        assert!(
            queue
                .try_slot(2)
                .expect("resident")
                .try_lane(1)
                .expect("destination")
                .set()
                == round::Insertion::Inserted
        );
        assert_eq!(
            round::Lane::try_new(&queue, 3)
                .expect("lane")
                .pop_front()
                .map(|slot| std::ptr::eq(slot, queue.try_slot(1).expect("resident").get())),
            Some(true)
        );
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(3));
    assert_eq!(drops::CALLS.with(cell::Cell::get), Some(0));
}

#[test]
fn callback_changes_front() {
    let queue = round::Lanes::<cell::Cell<usize>, 1>::try_new(2, num::NonZero::<usize>::MIN);
    let queue = queue.expect("required fixture outcome");
    for index in 0..2 {
        queue.try_slot(index).expect("resident").get().set(index);
    }
    let lane = round::Lane::try_new(&queue, 0).expect("lane");
    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(
        queue
            .try_slot(1)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    let front = lane.front();
    let front = front.expect("assertion failed: front.is_some()");
    assert!(
        front
            .pop_if(|slot| {
                assert_eq!(slot.get(), 0);
                assert_eq!(lane.pop_front().map(|slot| slot.get()), Some(0));
                true
            })
            .is_none()
    );
    assert_eq!(lane.pop_front().map(|slot| slot.get()), Some(1));
    assert!(lane.front().is_none());
}

#[test]
fn callbacks_recheck_order() {
    let queue = round::Lanes::<(), 1>::try_new(2, num::NonZero::<usize>::MIN.saturating_add(1))
        .expect("lanes");
    for index in 0..2 {
        queue
            .try_slot(index)
            .expect("resident")
            .try_lane(index)
            .expect("lane")
            .push();
    }
    let lane0 = round::Lane::try_new(&queue, 0).expect("lane");
    let lane1 = round::Lane::try_new(&queue, 1).expect("lane");
    let turn = queue.group::<0>().active().expect("turn");
    assert!(
        turn.pop_if(|_| {
            assert!(lane0.rotate().is_some());
            true
        })
        .is_none()
    );
    let front = lane0.front().expect("front");
    assert!(
        front
            .pop_if(|_| {
                assert!(lane1.pop_front().is_some());
                true
            })
            .is_some()
    );
}

#[test]
fn relinquishes_handles() {
    let created = round::Lanes::<u64, 1>::try_new(1, num::NonZero::<usize>::MIN);
    let mut queue = created.expect("required fixture outcome");
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
    let held = selected.dequeue();
    assert!(held.is_some());
    assert!(!queue.try_slot(0).expect("resident").is_linked());
    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Present
    );
    queue.try_slot(0).expect("resident").clear();
    assert!(queue.group::<0>().active().is_none());
}

#[test]
fn values_retain_detached() {
    let queue = values::Values::<num::NonZero<u64>, 2, 2>::try_fixed(1).expect("value owner");
    let slot = queue.try_slot(0).expect("resident");
    let first = slot.try_lane(0).expect("first lane");
    let second = slot.try_lane(1).expect("second lane");
    let value = num::NonZero::<u64>::MIN;

    assert!(slot.get().is_none());
    assert!(first.set(|_| Some(())).is_none());
    slot.write(value);
    slot.write(value.saturating_add(1));
    assert_eq!(slot.get(), Some(value.saturating_add(1)));
    assert!(!queue.has_pending());
    assert!(matches!(
        first.set(Some),
        Some((round::Insertion::Inserted, _))
    ));
    assert!(matches!(
        first.set(Some),
        Some((round::Insertion::Present, _))
    ));
    assert!(matches!(
        second.set(Some),
        Some((round::Insertion::Inserted, _))
    ));
    assert!(queue.group::<0>().active().is_none());
    assert!(queue.has_pending());
    assert_eq!(queue.group::<1>().next(), Some(value.saturating_add(1)));
    assert!(slot.get().is_none());

    first.push(value);
    slot.write(value.saturating_add(2));
    assert!(!queue.has_pending());
    assert_eq!(second.slot().take(), Some(value.saturating_add(2)));
    assert!(slot.take().is_none());
}

#[test]
fn values_recheck_callbacks() {
    let queue = values::Values::<u64, 1, 2>::try_fixed(2).expect("value owner");
    let first = queue.try_slot(0).expect("first resident");
    let second = queue.try_slot(1).expect("second resident");
    let lane0 = queue.try_lane(0).expect("first lane");
    let lane1 = queue.try_lane(1).expect("second lane");
    first.try_lane(0).expect("first destination").push(10);
    let (stale, value) = lane0.front().expect("initial front");
    assert_eq!(value, 10);
    assert_eq!(first.take(), Some(10));
    assert!(stale.pop_if(|_| panic!("stale callback")).is_none());
    first.try_lane(0).expect("first destination").push(10);
    second.try_lane(0).expect("second destination").push(20);

    let (front, _) = lane0.front().expect("first front");
    assert!(
        front
            .pop_if(|value| {
                assert_eq!(first.take(), Some(10));
                first.try_lane(1).expect("new destination").push(11);
                assert_eq!(value, 10);
                true
            })
            .is_none()
    );
    assert_eq!(
        lane0
            .front()
            .expect("remaining front")
            .0
            .pop_if(|value| value == 20),
        Some(20)
    );
    assert_eq!(lane1.pop_front(), Some(11));
}

#[test]
fn values_recheck_insertion() {
    let queue = values::Values::<u64, 1, 2>::try_fixed(1).expect("value owner");
    let slot = queue.try_slot(0).expect("resident");
    let first = slot.try_lane(0).expect("first lane");
    let second = slot.try_lane(1).expect("second lane");
    slot.write(10);
    assert!(first.set(|_| slot.take()).is_none());
    assert!(slot.get().is_none());
    assert!(!queue.has_pending());

    slot.write(10);
    assert!(
        first
            .set(|value| {
                second.push(11);
                Some(value)
            })
            .is_none()
    );
    assert!(queue.try_lane(0).expect("first lane").front().is_none());
    assert_eq!(
        queue.try_lane(1).expect("second lane").pop_front(),
        Some(11)
    );
}

#[test]
fn values_recheck_equality() {
    use common::depot::queue::Reentry;

    let queue = values::Values::<Reentry, 1, 1>::try_fixed(1).expect("value owner");
    let slot = queue.try_slot(0).expect("resident");
    // SAFETY: equality uses this pointer only while its owner remains alive and unmoved.
    slot.write(Reentry(&queue));
    assert!(slot.try_lane(0).expect("destination").set(Some).is_none());
    assert!(slot.get().is_none());
    assert!(!queue.has_pending());
}
