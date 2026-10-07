pub mod common;

use core::iter;
use std::{cell, mem};

use common::alloc::system::Tracker;
use o3::depot::{
    array::Index,
    capacity::Count,
    queue::group::{self, Ledger, capacity, drain::Drain, vacant},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn drains_only_owner_lane() {
    let capacity = capacity::Count::try_from(8).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 2>::try_new(capacity).expect("ledger allocation");
    for (owner, value) in [(1, 10), (2, 20), (1, 11), (3, 30), (1, 12)] {
        assert!(
            (ledger.push_back(Index::at::<0>(), owner, value)).is_ok(),
            "required fixture outcome"
        );
    }
    assert!(
        (ledger.push_back(Index::at::<1>(), 1, 99)).is_ok(),
        "required fixture outcome"
    );
    let mut released = Vec::new();
    assert!(ledger.drain_owner(1, Index::at::<0>(), |value| released.push(value)) == 3);
    assert_eq!(released, [10, 11, 12]);
    assert!(ledger.pop_front(Index::at::<0>()) == Some((2, 20)));
    assert!(ledger.pop_front(Index::at::<0>()) == Some((3, 30)));
    assert!(ledger.pop_front(Index::at::<1>()) == Some((1, 99)));
}
#[test]
fn restores_wire_position() {
    let capacity = capacity::Count::try_from(4).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 1>::try_new(capacity).expect("ledger allocation");
    let ((), counts) = Tracker::measure(|| {
        assert!(
            (ledger.push_back(Index::at::<0>(), 7, 70)).is_ok(),
            "required fixture outcome"
        );
        assert!(
            (ledger.push_back(Index::at::<0>(), 8, 80)).is_ok(),
            "required fixture outcome"
        );
        let (value, vacancy) = ledger
            .bind_front(Index::at::<0>())
            .map(group::Front::take)
            .expect("the seeded front must resolve");
        assert_eq!(value, 70);
        vacancy.restore(value);
        assert!(ledger.pop_front(Index::at::<0>()) == Some((7, 70)));
        assert!(ledger.pop_front(Index::at::<0>()) == Some((8, 80)));
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn forgets_owner_vacancy() {
    let capacity = capacity::Count::try_from(3).expect("tagged capacity");
    let mut ledger = Ledger::<Box<u64>, 1>::try_new(capacity).expect("ledger allocation");
    let lane = Index::at::<0>();
    for value in [10, 20] {
        assert!(ledger.push_back(lane, 7, Box::new(value)).is_ok());
    }
    for expected in [Some(20), None] {
        let (value, vacancy) = ledger.bind_front(lane).expect("queued front").take();
        mem::forget(vacancy);
        drop(value);
        assert_eq!(ledger.get(7, lane).map(|value| **value), expected);
        assert_eq!(ledger.lane_len(lane), usize::from(expected.is_some()));
    }
    assert!(ledger.push_back(lane, 7, Box::new(30)).is_ok());
    assert_eq!(ledger.pop_front(lane).map(|(_, value)| *value), Some(30));
}

#[test]
fn publishes_linked_batch() {
    let capacity = capacity::Count::try_from(6).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 2>::try_new(capacity).expect("ledger allocation");
    assert!(
        (ledger.push_back(Index::at::<0>(), 9, 1)).is_ok(),
        "required fixture outcome"
    );
    let ready = cell::Cell::new(false);
    let ((), activity) = Tracker::measure(|| {
        let slots = group::Batch::new(&mut ledger);
        let slots = slots.expect("the prepared batch must be available");
        let result = slots.publish(
            [
                vacant::Item::new(Index::at::<0>(), 9, 2),
                vacant::Item::new(Index::at::<1>(), 2, 3),
                vacant::Item::new(Index::at::<0>(), 9, 4),
                vacant::Item::new(Index::at::<0>(), 3, 5),
            ],
            &ready,
            |commit| commit.set(true),
            |value| {
                assert!(ready.get());
                value
            },
        );
        assert!(result.is_ok(), "required fixture outcome");
    });
    assert_eq!(activity, [0; 4]);
    let mut owner = Vec::new();
    ledger.visit_owner(9, Index::at::<0>(), |value| owner.push(*value));
    assert_eq!(owner, [1, 2, 4]);
    assert!(ledger.pop_front(Index::at::<1>()) == Some((2, 3)));
    for expected in [(9, 1), (9, 2), (9, 4), (3, 5)] {
        assert!(ledger.pop_front(Index::at::<0>()) == Some(expected));
    }
    assert!(ledger.is_empty());
    assert_eq!(ledger.available(), 6);
}

#[test]
fn rolls_back_batch() {
    let capacity = capacity::Count::try_from(4).expect("tagged capacity");
    let mut ledger = Ledger::<Box<u64>, 1>::try_new(capacity).expect("ledger allocation");
    let lane = Index::at::<0>();
    for key in [1, 2] {
        assert!(ledger.push_back(lane, key, Box::new(key)).is_ok());
        let mut drain = Drain::new(&mut ledger, key, lane).expect("owner");
        assert_eq!(*drain.next_entry().expect("entry").remove(), key);
        mem::forget(drain);
    }
    assert!(ledger.push_back(lane, 3, Box::new(3)).is_ok());
    let commit = String::from("retained commit");
    let pointer = commit.as_ptr();
    let items = [4, 5, 6].map(|key| vacant::Item::new(lane, key, Box::new(key)));
    let mut called = false;
    let result = group::Batch::new(&mut ledger)
        .expect("entry capacity")
        .publish(items, commit, |_| called = true, |value| value);
    assert!(!called);
    let (error, (commit, items)) = result.expect_err("owner capacity");
    assert_eq!(
        error,
        group::Error::Owners {
            lane,
            capacity,
            key: 5,
        }
    );
    assert_eq!(commit.as_ptr(), pointer);
    assert_eq!(
        items.map(|item| {
            let (lane, key, value) = item.into_parts();
            (lane, key, *value)
        }),
        [(lane, 4, 4), (lane, 5, 5), (lane, 6, 6)]
    );
    assert_eq!(ledger.available(), 3);
    assert_eq!(ledger.get(3, lane).map(|value| **value), Some(3));
    assert!(ledger.push_back(lane, 7, Box::new(7)).is_ok());
    assert_eq!(
        ledger.pop_front(lane).map(|(key, value)| (key, *value)),
        Some((3, 3))
    );
    assert_eq!(
        ledger.pop_front(lane).map(|(key, value)| (key, *value)),
        Some((7, 7))
    );
}

#[test]
fn clears_owned_slots() {
    let capacity = capacity::Count::try_from(6).expect("tagged capacity");
    let mut ledger = Ledger::<u64, 1>::try_new(capacity).expect("ledger allocation");
    let keys = [1, 128, 0, u64::MAX, 129, 1];
    for key in keys {
        assert!(
            (ledger.push_back(Index::at::<0>(), key, key)).is_ok(),
            "required fixture outcome"
        );
    }
    let mut released = Vec::with_capacity(keys.len());
    let ((), counts) = Tracker::measure(|| ledger.clear(|value| released.push(value)));
    assert_eq!(counts, [0; 4]);
    assert_eq!(released, keys);
    assert_eq!(ledger.lane_len(Index::at::<0>()), 0);
    assert_eq!(ledger.available(), keys.len());
    for key in keys {
        assert!(ledger.get(key, Index::at::<0>()).is_none());
        assert!(
            (ledger.push_back(Index::at::<0>(), key, key)).is_ok(),
            "required fixture outcome"
        );
        assert!(ledger.pop_front(Index::at::<0>()) == Some((key, key)));
    }
    ledger.clear(|value| released.push(value));
    assert_eq!(released, keys, "clearing an empty ledger releases nothing");
}
#[test]
fn commits_and_reuses_node() {
    let capacity = capacity::Count::try_from(1).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 1>::try_new(capacity).expect("ledger allocation");
    assert!(
        (ledger.push_back(Index::at::<0>(), 7, 70)).is_ok(),
        "required fixture outcome"
    );
    {
        let taken = ledger
            .bind_front(Index::at::<0>())
            .map(group::Front::take)
            .expect("the seeded front must resolve");
        assert_eq!(taken.0, 70);
    }
    assert_eq!(ledger.lane_len(Index::at::<0>()), 0);
    assert!(
        (ledger.push_back(Index::at::<0>(), 8, 80)).is_ok(),
        "required fixture outcome"
    );
    assert!(ledger.pop_front(Index::at::<0>()) == Some((8, 80)));
}
#[test]
fn proves_capacity_domain() {
    let maximum = Count::new(u32::MAX - 1);
    assert!(
        (capacity::Count::<2>::try_from(maximum)).is_ok(),
        "required fixture outcome"
    );
    let error = capacity::Count::<2>::try_from(Count::MAX)
        .expect_err("two lanes reserve an index beyond the maximum count");
    assert_eq!(error.requested(), u32::MAX as usize);
}

#[test]
fn rejects_lane_extent() {
    assert_eq!(
        capacity::Count::<{ usize::MAX }>::try_from(Count::new(1)),
        Err(capacity::Error::Tagged { requested: 1 }),
    );
    if usize::BITS > u32::BITS {
        assert!(matches!(
            capacity::Count::<{ usize::MAX }>::try_from(usize::MAX),
            Err(capacity::Error::Domain(error)) if error.requested() == usize::MAX
        ));
    }
}

#[test]
fn keeps_borrowed_address() {
    let source = String::from("lexical-owner");
    let pointer = source.as_ptr();
    let capacity = capacity::Count::try_from(1).expect("tagged capacity");
    let mut ledger = Ledger::<&str, 1>::try_new(capacity).expect("ledger allocation");
    assert!(
        (ledger.push_back(Index::at::<0>(), 1, source.as_str())).is_ok(),
        "required fixture outcome"
    );
    let (owner, retained) = ledger
        .pop_front(Index::at::<0>())
        .expect("the borrowed front must resolve");
    assert_eq!(owner, 1);
    assert_eq!(retained.as_ptr(), pointer);
    assert_eq!(retained, "lexical-owner");
}
#[test]
fn cancels_prepared_owners() {
    let capacity = capacity::Count::try_from(8).expect("tagged capacity");
    let mut ledger = Ledger::<u64, 1>::try_new(capacity).expect("ledger allocation");
    for key in [0, 128, 129] {
        assert!(
            (ledger.push_back(Index::at::<0>(), key, key)).is_ok(),
            "required fixture outcome"
        );
    }
    let before = ledger.available();
    let ((), usage) = Tracker::usage(|| {
        for key in [256, 64, 130, 129, 0, u64::MAX] {
            assert!(
                group::Batch::<_, 1, 1>::new(&mut ledger)
                    .and_then(|slot| slot.bind(Index::at::<0>(), key))
                    .is_ok(),
                "queue group must fit its declared capacity"
            );
            assert_eq!(ledger.available(), before);
            for key in [0, 128, 129] {
                assert!(ledger.get(key, Index::at::<0>()) == Some(&key));
            }
        }
    });
    assert_eq!(usage, (0, 0));
    for key in [0, 128, 129] {
        assert!(ledger.pop_front(Index::at::<0>()) == Some((key, key)));
    }
    assert!(ledger.is_empty());
    assert!(
        group::Batch::<_, 1, 1>::new(&mut ledger)
            .and_then(|slot| slot.bind(Index::at::<0>(), 42))
            .is_ok(),
        "queue group must fit its declared capacity"
    );
    assert!(ledger.get(42, Index::at::<0>()).is_none());
    assert_eq!(ledger.available(), 8);
    assert!(
        (ledger.push_back(Index::at::<0>(), 43, 43)).is_ok(),
        "required fixture outcome"
    );
    assert!(ledger.pop_front(Index::at::<0>()) == Some((43, 43)));
}
#[test]
fn preserves_sparse_lookup() {
    let capacity = capacity::Count::try_from(66).expect("tagged capacity");
    let mut ledger = Ledger::<u64, 1>::try_new(capacity).expect("ledger allocation");
    let keys: Vec<_> = iter::once(0)
        .chain((0..64).map(|bit| 1u64 << bit))
        .collect();
    for key in keys.iter().copied() {
        assert!(
            (ledger.push_back(Index::at::<0>(), key, key)).is_ok(),
            "required fixture outcome"
        );
    }
    for key in keys.iter().step_by(2).copied() {
        assert!(ledger.drain_owner(key, Index::at::<0>(), |value| assert_eq!(value, key)) == 1);
    }
    for key in keys.iter().skip(1).step_by(2).copied() {
        let mut seen = None;
        ledger.visit_owner(key, Index::at::<0>(), |value| seen = Some(*value));
        assert_eq!(seen, Some(key));
    }
    for key in keys.iter().skip(1).step_by(2).rev().copied() {
        assert!(ledger.drain_owner(key, Index::at::<0>(), |value| assert_eq!(value, key)) == 1);
    }
    assert_eq!(ledger.lane_len(Index::at::<0>()), 0);
}
#[test]
fn preserves_dense_chains() {
    let capacity = capacity::Count::try_from(512).expect("tagged capacity");
    let mut ledger = Ledger::<u64, 1>::try_new(capacity).expect("ledger allocation");
    let keys: Vec<_> = (0u64..512)
        .map(|key| key.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17))
        .collect();
    for key in keys.iter().copied() {
        assert!(
            (ledger.push_back(Index::at::<0>(), key, key)).is_ok(),
            "required fixture outcome"
        );
    }
    for key in keys.iter().rev().copied() {
        let mut seen = None;
        ledger.visit_owner(key, Index::at::<0>(), |value| seen = Some(*value));
        assert_eq!(seen, Some(key));
        assert!(ledger.drain_owner(key, Index::at::<0>(), |value| assert_eq!(value, key)) == 1);
    }
    assert_eq!(ledger.lane_len(Index::at::<0>()), 0);
    assert_eq!(ledger.available(), 512);
}
#[test]
fn drop_finishes_extraction() {
    let capacity = capacity::Count::try_from(3).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 2>::try_new(capacity).expect("ledger allocation");
    for value in 10..13 {
        assert!(
            (ledger.push_back(Index::at::<0>(), 7, value)).is_ok(),
            "required fixture outcome"
        );
    }
    {
        let taken = ledger
            .bind_front(Index::at::<0>())
            .map(group::Front::take)
            .expect("the seeded owner must resolve its first value");
        assert_eq!(taken.0, 10);
    }
    assert_eq!(ledger.lane_len(Index::at::<0>()), 2);
    assert_eq!(ledger.available(), 1);
    assert!(ledger.get(7, Index::at::<0>()) == Some(&11));
    let mut next = 11;
    assert!(
        ledger.drain_owner(7, Index::at::<0>(), |value| {
            assert_eq!(value, next);
            next += 1;
        }) == 2
    );
    assert_eq!(ledger.available(), 3);
    assert!(
        (ledger.push_back(Index::at::<1>(), 8, 80)).is_ok(),
        "required fixture outcome"
    );
    assert!(ledger.pop_front(Index::at::<1>()) == Some((8, 80)));
}
#[test]
fn cancels_owner_insertion() {
    for keys in [[0, 1, 1u64 << 63], [0, 1u64 << 63, 1u64 << 62], [0, 1, 0]] {
        let capacity = capacity::Count::try_from(3).expect("tagged capacity");
        let mut ledger = Ledger::<u64, 1>::try_new(capacity).expect("ledger allocation");
        for key in &keys[..2] {
            assert!(
                (ledger.push_back(Index::at::<0>(), *key, *key)).is_ok(),
                "required fixture outcome"
            );
        }
        let ((), counts) = Tracker::measure(|| {
            let vacant =
                group::Batch::<_, 1, 1>::new(&mut ledger).expect("the third slot can be reserved");
            let entry = vacant.bind(Index::at::<0>(), keys[2]);
            assert!(entry.is_ok());
        });
        assert_eq!(counts, [0; 4]);
        assert_eq!(ledger.available(), 1);
        assert_eq!(ledger.lane_len(Index::at::<0>()), 2);
        assert!(
            (ledger.push_back(Index::at::<0>(), keys[2], 99)).is_ok(),
            "required fixture outcome"
        );
        for key in &keys[..2] {
            assert!(ledger.pop_front(Index::at::<0>()) == Some((*key, *key)));
        }
        assert!(ledger.pop_front(Index::at::<0>()) == Some((keys[2], 99)));
        assert_eq!(ledger.available(), 3);
    }
    let capacity = capacity::Count::try_from(1).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 1>::try_new(capacity).expect("ledger allocation");
    assert!(
        group::Batch::<_, 1, 1>::new(&mut ledger)
            .and_then(|slot| slot.bind(Index::at::<0>(), 7))
            .is_ok(),
        "queue group must fit its declared capacity"
    );
    assert_eq!(ledger.available(), 1);
    assert!(ledger.get(7, Index::at::<0>()).is_none());
    assert!(
        (ledger.push_back(Index::at::<0>(), 8, 80)).is_ok(),
        "required fixture outcome"
    );
    assert!(ledger.pop_front(Index::at::<0>()) == Some((8, 80)));
}
