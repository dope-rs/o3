pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell, collections::VecDeque, mem};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{heap::EntryExt as _, model::Random, queue::Sequence},
};
use o3::{
    alloc,
    depot::{
        capacity,
        heap::indexed,
        map::Plan,
        queue::indexed::planes::{self, Priority as _, map},
    },
};

#[test]
fn admission() {
    let mut table = map::Table::<[u64; 1], u64, 3>::try_new(Plan::fixed::<2>(), 4).expect("table");
    let error = table.try_vacant(4, 9, [9]).err().expect("bounds");
    assert!(matches!(
        error,
        map::Error::Bounds {
            key: [9],
            index: 4,
            capacity: 4
        }
    ));
    table.try_vacant(1, 9, [9]).expect("first").insert(10);
    let error = table.try_vacant(1, 7, [7]).err().expect("occupied");
    assert!(matches!(
        error,
        map::Error::Occupied {
            key: [7],
            stored: [9],
            index: 1
        }
    ));
    let error = table.try_vacant(2, 9, [9]).err().expect("key");
    assert!(matches!(
        error,
        map::Error::Key {
            key: [9],
            stored: [9],
            index: 2
        }
    ));
    table.try_vacant(3, 7, [7]).expect("second").insert(20);
    let error = table.try_vacant(0, 8, [8]).err().expect("full");
    assert!(matches!(
        error,
        map::Error::Full {
            key: [8],
            capacity: 2
        }
    ));
    let first = table.at(1).expect("first slot");
    assert_eq!((first.key()[0], **first), (9, 10));
    let second = table.at(3).expect("second slot");
    assert_eq!((second.key()[0], **second), (7, 20));
    let mut empty = map::Table::<[u64; 1], (), 1>::try_new(Plan::fixed::<0>(), 1).expect("empty");
    assert!(matches!(
        empty.try_vacant(0, 0, [0]),
        Err(map::Error::Full {
            key: [0],
            capacity: 0
        })
    ));
}

#[test]
fn collisions_and_planes() {
    const COUNT: usize = 31;
    let mut table =
        map::Table::<[u64; 1], u64, 3>::try_new(Plan::fixed::<COUNT>(), COUNT).expect("table");
    let mut live = VecDeque::new();
    let mut ready = VecDeque::new();
    let mut rng = Random::<u64, 13, 7, 17>::try_new(0x9329_157b_4723_8841_u64).expect("shifts");
    for step in 0..20_000 {
        let seed = rng.advance();
        let key = seed % COUNT as u64;
        if step % 11 == 0 {
            if let Some(entry) = table.first_entry() {
                let (key, value) = entry.remove();
                assert_eq!(Some(key[0]), live.pop_front());
                assert!(value >= key[0]);
                ready.retain(|stored| *stored != key[0]);
                assert!(table.get(0, &key[..]).is_none());
            }
        } else if step % 5 == 0 {
            if let Some(front) = table.front_entry::<1>() {
                let mut entry = front.pop();
                assert_eq!(Some(entry.key()[0]), ready.pop_front());
                *entry += 1;
                entry.plane::<2>().link_back();
            }
        } else if let Some(mut entry) = table.entry(0, &[key][..]) {
            if step % 3 == 0 {
                let (removed, value) = entry.remove();
                assert_eq!(removed, [key]);
                assert!(value >= key);
                live.retain(|stored| *stored != key);
                ready.retain(|stored| *stored != key);
            } else {
                entry.plane::<1>().link_back();
                if !ready.contains(&key) {
                    ready.push_back(key);
                }
            }
        } else {
            let mut entry = table
                .try_vacant(key as usize, 0, [key])
                .expect("free slot")
                .insert(key);
            entry.plane::<1>().link_back();
            live.push_back(key);
            ready.push_back(key);
        }
        assert_eq!(
            table.front::<0>().map(|record| {
                assert!(**record >= record.key()[0]);
                record.key()[0]
            }),
            live.front().copied()
        );
        assert_eq!(
            table.front::<1>().map(|record| {
                assert!(**record >= record.key()[0]);
                record.key()[0]
            }),
            ready.front().copied()
        );
        for key in &live {
            assert!(table.get(0, &[*key][..]).is_some());
        }
    }
}

#[test]
fn storage_contract() {
    assert_eq!(mem::size_of::<map::Table<[u64; 1], u64, 3>>(), 96);
    assert_eq!(mem::size_of::<map::Occupied<'_, [u64; 1], u64, 3>>(), 16);
    assert_eq!(
        mem::size_of::<Option<map::Occupied<'_, [u64; 1], u64, 3>>>(),
        16
    );
    assert_eq!(
        mem::size_of::<
            map::Occupied<'_, [u64; 1], u64, 3, o3::depot::queue::indexed::planes::phase::Front<1>>,
        >(),
        16
    );
    assert_eq!(
        mem::size_of::<
            map::Slot<'_, [u64; 1], u64, 3, o3::depot::queue::indexed::planes::phase::Vacant<1>>,
        >(),
        16
    );
    let drops = cell::Cell::new(Some(0));
    {
        let (mut table, allocs) = Tracker::measure(|| {
            map::Table::<_, _, 3>::try_new(Plan::fixed::<4>(), 100).expect("table")
        });
        assert_eq!(allocs, [2, 4_904, 0, 0]);
        let ((), allocs) = Tracker::measure(|| {
            for key in 0..4_u64 {
                let mut entry = table
                    .try_vacant(key as usize, 0, [key])
                    .expect("slot")
                    .insert(Counted(&drops));
                entry.plane::<1>().link_back();
                entry.plane::<2>().link_back();
            }
            {
                let entry = table.front_entry::<1>().expect("front").pop();
                let removed = entry.remove();
                assert_eq!(removed.0, [0]);
            }
            assert_eq!(drops.get(), Some(1));
        });
        assert_eq!(allocs, [0, 0, 0, 0]);
    }
    assert_eq!(drops.get(), Some(4));
}

#[test]
fn fixed_storage_admission() {
    let (empty, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            map::Table::<[u64; 1], (), 1>::try_new(Plan::fixed::<0>(), 0)
        })
    });
    let mut empty = empty.expect("empty storage needs no allocation");
    assert_eq!(activity, [0, 0, 0, 0]);
    assert!(empty.at(0).is_none());
    assert!(empty.slot(0).is_none());
    assert!(empty.front::<0>().is_none());
    let ((), activity) = Tracker::measure(move || {
        assert!(matches!(
            empty.try_vacant(0, 0, [0]),
            Err(map::Error::Bounds { capacity: 0, .. })
        ));
    });
    assert_eq!(activity, [0, 0, 0, 0]);

    let (overflow, activity) =
        Tracker::measure(|| map::Table::<[u64; 1], (), 1>::try_new(Plan::fixed::<1>(), usize::MAX));
    assert!(matches!(overflow, Err(alloc::Error::Overflow)));
    assert_eq!(activity, [0, 0, 0, 0]);

    for allowed in [0, 1] {
        let (failed, activity) = Tracker::measure(|| {
            Tracker::fail_after(allowed, || {
                map::Table::<[u64; 1], (), 1>::try_new(Plan::fixed::<2>(), 4)
            })
        });
        assert!(matches!(failed, Err(alloc::Error::Exhausted)));
        let [count, bytes, freed, released] = activity;
        assert_eq!(count, allowed);
        assert_eq!(freed, allowed);
        assert_eq!(bytes, released);
    }
}

#[test]
fn preserves_domain() {
    let (result, allocs) = Tracker::counts(|| {
        let seed = Sequence::try_new(10).expect("factor");
        planes::Set::<usize, 2, indexed::Min<u64>>::try_seed(capacity::Count::new(8), seed)
    });
    let mut heap = (result).expect("queue planes must retain their allocation domain");

    assert_eq!(allocs, [2, 0]);
    let ((), allocs) = Tracker::measure(|| {
        assert!(heap.select(8).is_none());
        for index in 0..8 {
            let (timer, mut slot) = heap.select(index).expect("the slot exists");
            assert_eq!(*slot, index * 10);
            slot.plane::<0>().link_back();
            let timer = timer.into_vacant().expect("initial priority is vacant");
            timer.set((8 - index) as u64);
        }
        let (timer, slot) = heap.first().expect("the minimum retains its payload");
        assert_eq!(*slot, 70);
        assert_eq!(*timer.priority(), 1);
        timer.set(20);
    });
    assert_eq!(allocs, [0, 0, 0, 0]);
    assert!(
        (heap
            .try_grow_to(
                capacity::Count::new(16),
                Sequence::try_new(10).expect("factor")
            )
            .expect("queue planes must retain their allocation domain"))
            == (8..16)
    );
    assert_eq!(heap.len_in::<0>(), 8);
    for index in 8..16 {
        let (timer, slot) = heap.select(index).expect("new domain exists");
        assert_eq!(*slot, index * 10);
        assert!(matches!(timer, indexed::Entry::Vacant(_)));
    }
    for index in (0..7).rev().chain([7]) {
        let (timer, mut slot) = heap.first().expect("every old priority survives growth");
        assert_eq!(slot.index(), index);
        slot.plane::<0>().unlink();
        timer.retire();
    }
    assert!(heap.first().is_none());
    assert_eq!(heap.len_in::<0>(), 0);
}

#[test]
fn preserves_failed_growth() {
    let mut heap = (planes::Set::<usize, 2, indexed::Min<u64>>::try_seed(
        capacity::Count::new(8),
        Sequence::try_new(10).expect("factor"),
    ))
    .expect("queue planes must retain their allocation domain");

    let (timer, mut slot) = heap.select(3).expect("represented slot");
    slot.plane::<0>().link_back();
    let timer = timer.into_vacant().expect("initial priority is vacant");
    timer.set(19);
    for allows in [0, 1] {
        let error = Tracker::fail_after(allows, || {
            heap.try_grow_to(
                capacity::Count::new(64),
                Sequence::try_new(10).expect("factor"),
            )
        })
        .expect_err("one allocation is rejected");
        if allows == 0 {
            assert!(
                matches!(error, planes::Error::Heap(o3::depot::heap::Error::Allocation { capacity, .. }) if capacity.get() == 64)
            );
        } else {
            assert!(
                matches!(error, planes::Error::Values { capacity, error: alloc::Error::Exhausted } if capacity.get() == 64)
            );
        }
        assert_eq!(heap.len(), 8);
        assert_eq!(heap.len_in::<0>(), 1);
        let (timer, slot) = heap.first().expect("failed growth retains the minimum");
        assert_eq!(slot.index(), 3);
        assert_eq!(*slot, 30);
        assert_eq!(*timer.priority(), 19);
    }
}

#[test]
fn resets_entries() {
    let capacity = capacity::Count::new(8);
    let (result, allocs) =
        Tracker::counts(|| planes::Set::<u64, 2, indexed::Min<u64>>::try_repeat(capacity, 17));
    let mut heap = (result).expect("queue planes must retain their allocation domain");

    assert_eq!(allocs, [2, 0]);
    let ((), allocs) = Tracker::measure(|| {
        for index in 0..8 {
            let (timer, mut slot) = heap.select(index).expect("represented endpoint");
            let indexed::Entry::Occupied(timer) = timer else {
                panic!("initial deadline is resident");
            };
            assert_eq!(*timer.priority(), 17);
            *slot = index as u64 + 1;
            slot.plane::<0>().link_back();
        }
        for value in heap.reset() {
            *value *= 2;
        }
        assert!(heap.first().is_none());
        for index in 0..8 {
            let slot = heap
                .front_entry::<0>()
                .expect("reset preserves event order")
                .pop();
            assert_eq!(slot.index(), index);
            assert_eq!(*slot, 2 * (index as u64 + 1));
        }
        let (timer, slot) = heap.select(3).expect("reset preserves endpoint domain");
        let timer = timer.into_vacant().expect("reset removes the deadline");
        assert_eq!(*slot, 8);
        timer.set(23);
        let (timer, slot) = heap.first().expect("rearming retains the endpoint");
        assert_eq!(*slot, 8);
        assert_eq!(*timer.priority(), 23);
    });
    assert_eq!(allocs, [0, 0, 0, 0]);
    for allows in [0, 1] {
        let error = Tracker::fail_after(allows, || {
            planes::Set::<u64, 2, indexed::Min<u64>>::try_repeat(capacity, 17)
        })
        .err()
        .expect("construction fails atomically");
        if allows == 0 {
            assert!(
                matches!(error, planes::Error::Heap(o3::depot::heap::Error::Allocation { capacity, .. }) if capacity.get() == 8)
            );
        } else {
            assert!(
                matches!(error, planes::Error::Values { capacity, error: alloc::Error::Exhausted } if capacity.get() == 8)
            );
        }
    }
}
