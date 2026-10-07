pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{array, cell, collections::VecDeque};

use common::{
    alloc::system::{Activity, Tracker},
    depot::model::Lcg,
};
use o3::depot::{index::Initialized, queue::indexed::shared, raw::Proof};

#[test]
fn rejects_bounds() {
    for capacity in [0, 1, 3] {
        let queue = shared::Fifo::<u64>::try_new(capacity).expect("queue");
        for index in [capacity, usize::MAX] {
            let error = queue.try_entry(index).err().expect("outside allocation");
            assert_eq!(error, shared::Bounds { index, capacity });
        }
        assert!(queue.next::<0>().is_none());
    }
}

#[test]
fn retains_slots() {
    let queue = shared::Fifo::<u64, cell::Cell<u64>>::try_new(7).expect("queue");
    let slots: [_; 7] = array::from_fn(|index| queue.try_entry(index).expect("slot"));
    let mut pending = VecDeque::new();
    let mut values = [None; 7];
    let mut rng = Lcg::<1>::new(41_u64);
    for step in 0..50_000 {
        let state = rng.advance();
        let index = (state >> 32) as usize % slots.len();
        match state % 4 {
            0 | 1 => {
                pending.retain(|&old| old != index);
                pending.push_back(index);
                values[index] = Some(step);
                slots[index].push::<0>(step);
                slots[index].state().set(step);
            }
            2 => {
                pending.retain(|&old| old != index);
                values[index] = None;
                slots[index].clear::<0>();
            }
            _ => match pending.pop_front() {
                Some(index) => {
                    let (slot, value) = queue.next::<0>().expect("pending value");
                    assert_eq!(slot.index(), index);
                    assert_eq!(Some(value), values[index].take());
                    assert_eq!(slot.state().get(), value);
                    assert!(slot.get::<0>().is_none());
                }
                None => assert!(queue.next::<0>().is_none()),
            },
        }
        for (slot, expected) in slots.iter().zip(values) {
            assert_eq!(slot.get::<0>(), expected);
        }
    }
}

#[test]
fn retains_snapshots() {
    let queue = shared::Fifo::<u64, cell::Cell<u64>>::try_new(1).expect("queue");
    let first = queue.try_entry(0).expect("first borrow");
    // SAFETY: first retains this initialized physical slot in the same queue.
    let second = unsafe { Proof::<_, Initialized<()>>::new((&queue, first.index())) }.into();
    first.push::<0>(11);
    let snapshot = second.get::<0>().expect("snapshot");
    first.clear::<0>();
    second.push::<0>(22);
    assert_eq!(snapshot, 11);
    assert_eq!(first.get::<0>(), Some(22));
    let state = first.state();
    state.set(33);
    let (slot, value) = queue.next::<0>().expect("replacement");
    assert_eq!((value, slot.state().get()), (22, 33));
}

#[test]
fn keeps_planes_independent() {
    let queue = shared::Fifo::<u64, cell::Cell<u64>, 2>::try_new(3).expect("resident queues");
    let first = queue.try_entry(0).expect("first");
    let second = queue.try_entry(1).expect("second");
    let last = queue.try_entry(2).expect("last");
    for epoch in 0..1_000 {
        first.push::<0>(epoch);
        second.push::<0>(epoch + 1);
        last.push::<0>(epoch + 2);
        last.push::<1>(epoch + 3);
        first.push::<1>(epoch + 4);
        second.push::<1>(epoch + 5);
        first.clear::<0>();
        last.push::<1>(epoch + 6);
        assert_eq!(first.get::<1>(), Some(epoch + 4));
        let (slot, value) = queue.next::<0>().expect("idle second");
        assert_eq!((slot.index(), value), (1, epoch + 1));
        slot.state().set(epoch);
        let (slot, value) = queue.next::<1>().expect("deferred first");
        assert_eq!((slot.index(), value), (0, epoch + 4));
        let (slot, value) = queue.next::<1>().expect("deferred second");
        assert_eq!(
            (slot.index(), value, slot.state().get()),
            (1, epoch + 5, epoch)
        );
        let (slot, value) = queue.next::<0>().expect("idle last");
        assert_eq!((slot.index(), value), (2, epoch + 2));
        assert_eq!(slot.get::<1>(), Some(epoch + 6));
        slot.clear::<1>();
        assert!(queue.next::<0>().is_none());
        assert!(queue.next::<1>().is_none());
    }
}

#[test]
fn shared_allocates_once() {
    let freed = {
        let (created, cold) = Tracker::measure(|| shared::Fifo::<u64, cell::Cell<u64>>::try_new(4));
        let queue = created.expect("four resident slots");
        assert_eq!(cold, [1, 96, 0, 0]);
        let slot = queue.try_entry(2).expect("admitted slot");
        let ((), hot) = Tracker::measure(|| {
            for value in 0..1_000 {
                slot.push::<0>(value);
                slot.state().set(value);
                let (taken, observed) = queue.next::<0>().expect("queued slot");
                assert_eq!(
                    (taken.index(), observed, taken.state().get()),
                    (2, value, value)
                );
                taken.push::<0>(value + 1);
                slot.clear::<0>();
            }
        });
        assert_eq!(hot, [0; 4]);
        Tracker::begin()
    };
    let freed = freed.finish();
    assert_eq!(
        freed,
        Activity {
            deallocations: 1,
            deallocated_bytes: 96,
            ..Activity::default()
        }
    );
}

#[test]
fn shared_drops_state() {
    let freed = {
        let queue =
            shared::Fifo::<u64, cell::RefCell<Vec<u8>>>::try_new(2).expect("resident state");
        *queue.try_entry(0).expect("first slot").state().borrow_mut() = vec![1; 32];
        let slot = queue.try_entry(1).expect("second slot");
        *slot.state().borrow_mut() = vec![2; 64];
        slot.push::<0>(5);
        Tracker::begin()
    };
    let freed = freed.finish();
    assert_eq!(
        freed,
        Activity {
            deallocations: 3,
            deallocated_bytes: 192,
            ..Activity::default()
        }
    );
}

#[test]
fn shared_planes_allocate() {
    let freed = {
        let (created, cold) =
            Tracker::measure(|| shared::Fifo::<u64, cell::Cell<u64>, 2>::try_new(4));
        let queue = created.expect("co-owned planes");
        assert_eq!(cold, [1, 160, 0, 0]);
        let slot = queue.try_entry(2).expect("admitted slot");
        let ((), hot) = Tracker::measure(|| {
            for value in 0..1_000 {
                slot.push::<0>(value);
                slot.push::<1>(value + 1);
                let (taken, observed) = queue.next::<0>().expect("first plane");
                assert_eq!((taken.index(), observed), (2, value));
                assert_eq!(taken.get::<1>(), Some(value + 1));
                taken.clear::<1>();
            }
        });
        assert_eq!(hot, [0; 4]);
        Tracker::begin()
    };
    let freed = freed.finish();
    assert_eq!(
        freed,
        Activity {
            deallocations: 1,
            deallocated_bytes: 160,
            ..Activity::default()
        }
    );
}
