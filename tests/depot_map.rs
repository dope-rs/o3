pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, mem, rc::Rc};

use common::{
    alloc::system::Tracker,
    depot::{
        map::{Aligned, AlignedSeed, Collision, Model, Seed},
        model::{Random, drops::Tracked},
    },
};
use o3::depot::{
    capacity::Count,
    map::{
        self, Plan,
        flat::{Entry, Table, errors::Insert},
        priority,
    },
    raw::Proof,
};

#[test]
fn keeps_element_bounds() {
    if usize::BITS == 64 {
        let limit = 1_usize << 30;
        let excess = limit + 1;
        priority::Plan::<u8, u8, u8>::new(limit)
            .as_ref()
            .expect("required fixture outcome");
        let expected = Plan::new(excess).err().expect("domain rejection");
        assert_eq!(expected.requested(), excess);
        assert!(
            matches!(priority::Plan::<u8, u8, u8>::new(excess), Err(map::Capacity::Domain(error)) if error == expected)
        );
    }

    let plan = Plan::fixed::<{ 1 << 30 }>();
    let (flat, counts) =
        Tracker::measure(|| Table::<[u8; 1 << (usize::BITS / 2 + 2)]>::try_new(plan));
    assert!(matches!(flat, Err(o3::alloc::Error::Overflow)));
    assert_eq!(counts, [0; 4]);
}

#[test]
fn allocations_are_exact() {
    let shape = Plan::fixed::<64>();
    let (freed, bytes) = {
        let (map, alloc) = Tracker::measure(|| Table::try_new(shape));
        let mut map = map.expect("the sixty-four-slot flat table must allocate");
        let [allocs, bytes, frees, freed] = alloc;
        assert_eq!((allocs, frees, freed), (1, 0, 0));
        assert!(bytes >= 64 * mem::size_of::<u64>());
        let ((), hot) = Tracker::measure(|| {
            for key in 0..64_u64 {
                map.try_insert(key & 7, key, |stored| *stored == key)
                    .as_ref()
                    .expect("required fixture outcome");
            }
            for key in 0..64_u64 {
                assert_eq!(map.get(key & 7, |stored| *stored == key), Some(&key));
            }
            for key in 0..64_u64 {
                assert_eq!(map.remove(key & 7, |stored| *stored == key), Some(key));
            }
        });
        assert_eq!(hot, [0; 4]);
        (Tracker::begin(), bytes)
    };
    assert_eq!(
        freed.finish(),
        common::alloc::system::Activity {
            deallocations: 1,
            deallocated_bytes: bytes,
            ..Default::default()
        }
    );
}

#[test]
fn fingerprints_remain() {
    let plan = Plan::fixed::<2>();
    let mut map = Table::try_new(plan).expect("the two-slot fingerprint table must allocate");
    let compared = Cell::new(false);
    let first = map.try_insert(u64::MAX, 1, |_| {
        compared.set(true);
        false
    });
    first.as_ref().expect("required fixture outcome");
    assert!(!compared.get());
    map.try_insert(0, 2, |stored| *stored == 2)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(map.get(u64::MAX, |value| *value == 1), Some(&1));
    assert_eq!(map.remove(u64::MAX, |value| *value == 1), Some(1));
    assert_eq!(map.get(0, |value| *value == 2), Some(&2));
}

#[test]
fn retains_flat_admission() {
    let mut table = Table::<u32>::try_new(Plan::fixed::<2>()).expect("table");
    for value in [1, 1, 2] {
        // SAFETY: there is one unused logical slot before every query, including the duplicate.
        let source = unsafe { Proof::new((&mut table, value)) };
        let (entry, value) = Table::admit(source, 7, |stored| *stored == value);
        match entry {
            Entry::Occupied(entry) => assert_eq!(entry.get(), &value),
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
        }
    }
    assert_eq!(table.remove(7, |value| *value == 1), Some(1));
    // SAFETY: backshift preserves the remaining entry's stored hash and value;
    // the exclusive table loan retains it through this exact immutable matcher.
    let source = unsafe {
        Proof::new((&mut table, String::from("held"), 7, |value: &u32| {
            *value == 2
        }))
    };
    let (entry, held) = Table::member(source);
    assert_eq!((entry.remove(), held.as_str()), (2, "held"));
}

#[test]
fn restores_flat_slot() {
    let mut table = Table::try_new(Plan::fixed::<2>()).expect("table");
    for value in [1, 2] {
        table.try_insert(7, value, |_| false).expect("capacity");
    }
    assert_eq!(table.remove(7, |value| *value == 1), Some(1));
    // SAFETY: removal released one logical entry in this exact retained table.
    let source = unsafe { Proof::new((&mut table, String::from("held"))) };
    let (value, held) = Table::insert(source, 7, 3);
    assert_eq!((*value, held.as_str()), (3, "held"));
    assert_eq!(table.len(), 2);
    assert_eq!(table.remove(7, |value| *value == 2), Some(2));
    assert_eq!(table.remove(7, |value| *value == 3), Some(3));
}

#[test]
fn binds_probe_to_mutation() {
    let plan = Plan::fixed::<1>();
    let mut table = (Table::try_new(plan)).expect("the one-slot flat table must allocate");

    let compared = Cell::new(false);
    let Some(Entry::Vacant(entry)) = table.entry(7, |_| compared.replace(true)) else {
        panic!("the first probe must find the sole slot vacant");
    };
    entry.insert(String::from("value"));
    assert!(!compared.get());
    assert!(matches!(
        table.try_insert(7, String::from("duplicate"), |value| value == "value"),
        Err(Insert::Occupied(value)) if value == "duplicate"
    ));

    let Some(Entry::Occupied(entry)) = table.entry(7, |value| value == "value") else {
        panic!("inserted probe remains occupied");
    };
    entry.into_mut().push_str(" probed");

    assert!(table.entry(9, |_| false).is_none());
    table
        .get_mut(7, |value| value == "value probed")
        .expect("inserted value")
        .push_str(" direct");

    let mut entry = (table.occupied(7, |value| value == "value probed direct"))
        .expect("the inserted probe remains occupied");
    entry.get_mut().push_str(" updated");
    assert_eq!(entry.get(), "value probed direct updated");
    assert_eq!(entry.remove(), "value probed direct updated");
    assert_eq!(table.values().count(), 0);

    let Some(Entry::Vacant(entry)) = table.entry(11, |_| false) else {
        panic!("removal must restore the sole slot's vacancy");
    };
    entry.insert(String::from("reused"));
    assert_eq!(table.get(11, |_| true).map(String::as_str), Some("reused"));
    assert_eq!(
        table.values().map(String::as_str).collect::<Vec<_>>(),
        ["reused"]
    );
}

#[test]
fn reuses_wrapped_clusters() {
    let plan = Plan::fixed::<8>();
    let error = Plan::new(usize::MAX).err().expect("index domain overflow");
    assert_eq!(error.requested(), usize::MAX);
    assert!((plan.capacity()) == (Count::new(8)));
    let mut table =
        (Table::<(u32, u32)>::try_new(plan)).expect("the eight-slot flat table must allocate");
    for epoch in 0..256u32 {
        for key in 0..8u32 {
            assert!(table.try_insert(15, (epoch, key), |entry| entry.1 == key) == Ok(()));
        }
        if epoch % 2 == 0 {
            table.clear();
        } else {
            for key in [3, 0, 7, 1, 6, 2, 5, 4] {
                assert_eq!(table.remove(15, |entry| entry.1 == key), Some((epoch, key)));
            }
        }
        assert_eq!(table.values().count(), 0);
    }
}

#[test]
fn matcher_releases_capture() {
    let mut table = Table::try_new(Plan::fixed::<1>()).expect("table");
    let life = Rc::new(());
    let held = Rc::clone(&life);
    assert_eq!(
        table.try_insert(7, 11, move |_| {
            assert_eq!(Rc::strong_count(&held), 2);
            false
        }),
        Ok(())
    );
    assert_eq!(Rc::strong_count(&life), 1);

    for matches in [true, false] {
        let held = Rc::clone(&life);
        let result = table.try_insert(7, 12, move |_| {
            assert_eq!(Rc::strong_count(&held), 2);
            matches
        });
        assert_eq!(
            result,
            Err(if matches {
                Insert::Occupied(12)
            } else {
                Insert::Full(12)
            })
        );
        assert_eq!(Rc::strong_count(&life), 1);

        let held = Rc::clone(&life);
        let found = table.get(7, move |_| {
            assert_eq!(Rc::strong_count(&held), 2);
            matches
        });
        assert_eq!(found, matches.then_some(&11));
        assert_eq!(Rc::strong_count(&life), 1);
    }
    assert_eq!(table.len(), 1);
    assert_eq!(table.get(7, |_| true), Some(&11));
}

#[test]
fn clear_restores_table() {
    let drops = [Cell::new(Some(0)), Cell::new(Some(0)), Cell::new(Some(0))];
    {
        let plan = Plan::fixed::<2>();
        let mut table = (Table::<Tracked<'_>>::try_new(plan))
            .expect("the two-slot clearable flat table must allocate");
        for index in 0..2 {
            let value = Tracked::try_new(index, &drops).expect("drop counter exists");
            table
                .try_insert(u64::from(index), value, |_| false)
                .expect("available slot");
        }
        table.clear();
        table.clear();
        assert_eq!(table.values().count(), 0);
        assert_eq!(drops.each_ref().map(Cell::get), [1, 1, 0].map(Some));
        let value = Tracked::try_new(2, &drops).expect("drop counter exists");
        table.try_insert(2, value, |_| false).expect("cleared slot");
        assert_eq!(table.values().count(), 1);
        assert!(table.get(2, |_| true).is_some());
        assert_eq!(drops.each_ref().map(Cell::get), [1, 1, 0].map(Some));
    }
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 3]);
}

#[test]
fn priority_drops_values() {
    let drops = [Cell::new(Some(0)), Cell::new(Some(0))];
    {
        let plan = priority::Plan::new(2).expect("plan");
        let mut map = priority::Min::try_new(plan, Seed(0)).expect("map");
        for key in 0_u64..2 {
            let value = Tracked::try_new(key as u8, &drops).expect("counter");
            let priority::Entry::Vacant(entry) = map.try_entry(key).expect("capacity") else {
                panic!("occupied");
            };
            entry.insert(key, value);
        }
        drop(map.remove(&0).expect("member"));
    }
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn moves_only_indices() {
    let result = priority::Plan::<u64, _, _>::new(4);
    let plan = result.expect("four entries must fit the priority-map storage layout");
    let (result, counts) = Tracker::measure(|| priority::Min::try_new(plan, Seed(0x71)));
    let mut map = result.expect("the four-entry priority map must allocate");
    assert_eq!(counts[0], 1);
    assert_eq!(mem::size_of_val(&map), 72);

    let ((), counts) = Tracker::measure(|| {
        for (key, rank, value) in [(10, 30, 100), (20, 20, 200)] {
            let priority::Entry::Vacant(entry) = map.try_entry(key).expect("capacity") else {
                panic!("occupied");
            };
            entry.insert(rank, value);
        }
        let address =
            (map.get(&10).map(|value| value as *const _)).expect("inserted priority value");
        let mut entry = (map.occupied(&10)).expect("inserted priority key must be occupied");
        *entry.get_mut() += 1;
        assert_eq!(entry.priority(), &30);
        entry.set(5);
        assert_eq!(entry.priority(), &5);
        let priority::Entry::Vacant(entry) = map.try_entry(40).expect("capacity") else {
            panic!("occupied");
        };
        entry.insert(40, 400);
        assert_eq!(map.get(&10).map(|value| value as *const _), Some(address));
        assert_eq!(map.len(), 3);
        assert_eq!(map.peek(), Some((&10, &5, &101)));
        assert_eq!(map.pop(), Some((10, 5, 101)));
        assert_eq!(map.remove(&20), Some(200));
        assert_eq!(map.pop(), Some((40, 40, 400)));
    });
    assert_eq!(counts, [0, 0, 0, 0]);
}

#[test]
fn keeps_high_alignment() {
    const CAPACITY: usize = 3;
    const BYTES: usize = 960;

    type Plan = priority::Plan<Aligned, Aligned, Aligned>;
    assert_eq!(mem::size_of::<Plan>(), mem::size_of::<u32>());
    let result = Plan::new(CAPACITY);
    let plan = result.expect("three aligned entries must fit the priority-map storage layout");
    let (result, counts) = Tracker::measure(|| priority::Min::try_new(plan, AlignedSeed));
    let mut map = result.expect("the aligned three-entry map must allocate");
    assert_eq!(counts, [1, BYTES, 0, 0]);
    let Ok(priority::Entry::Vacant(entry)) = map.try_entry(Aligned(11)) else {
        panic!("vacant");
    };
    entry.insert(Aligned(7), Aligned(13));
    let (key, priority, value) = map.peek().expect("inserted priority entry");
    assert_eq!((key.0, priority.0, value.0), (11, 7, 13));
    assert_eq!((key as *const Aligned).addr() % 64, 0);
    assert_eq!((priority as *const Aligned).addr() % 64, 0);
    assert_eq!((value as *const Aligned).addr() % 64, 0);

    let ((), release) = Tracker::measure(|| drop(map));
    assert_eq!(release, [0, 0, 1, BYTES]);
}

#[test]
fn matches_collision_churn() {
    use priority::Entry;

    const CAPACITY: usize = 23;
    const STEPS: usize = 50_000;

    let result = priority::Plan::new(CAPACITY);
    let plan = result.expect("churn capacity must fit the priority-map storage layout");
    let result = priority::Min::try_new(plan, Collision);
    let mut map = result.expect("the 23-entry collision map must allocate");
    let mut model = Vec::<Model>::with_capacity(CAPACITY);
    let mut rng = Random::<u64, 7, 9, 8>::try_new(0xa076_1d64_78bd_642f_u64).expect("shifts");

    for step in 0..STEPS {
        let random = rng.advance();
        let key = random.rotate_left(23) % 53;
        match random % 5 {
            0 => {
                let value = step as u64;
                let priority = (random, value);
                let found = model.iter().any(|entry| entry.key == key);
                match map.try_entry(key) {
                    Ok(Entry::Occupied(entry)) => {
                        assert!(found);
                        assert_eq!(*entry.key(), key);
                    }
                    Err(rejected) => {
                        assert!(!found);
                        assert_eq!(model.len(), CAPACITY);
                        assert_eq!(rejected.into_parts(), (key, priority::errors::Full));
                    }
                    Ok(Entry::Vacant(entry)) => {
                        assert!(!found);
                        assert!(model.len() < CAPACITY);
                        entry.insert(priority, value);
                        model.push(Model {
                            key,
                            value,
                            priority,
                        });
                    }
                }
            }
            1 => {
                let expected = model
                    .iter()
                    .position(|entry| entry.key == key)
                    .map(|index| model.swap_remove(index).value);
                assert_eq!(map.remove(&key), expected);
            }
            2 => {
                let expected = model
                    .iter_mut()
                    .find(|entry| entry.key == key)
                    .map(|entry| {
                        let next = (random.rotate_left(11), step as u64);
                        mem::replace(&mut entry.priority, next)
                    });
                let occupied = match map.try_entry(key) {
                    Ok(Entry::Occupied(entry)) => Some(entry),
                    Ok(Entry::Vacant(_)) | Err(_) => None,
                };
                if let Some(expected) = expected {
                    let mut entry = occupied
                        .expect("a key present in the churn model must be occupied in the map");
                    let next = (random.rotate_left(11), step as u64);
                    assert_eq!(entry.priority(), &expected);
                    entry.set(next);
                } else {
                    assert!(occupied.is_none());
                }
            }
            3 => {
                let index = model
                    .iter()
                    .enumerate()
                    .min_by_key(|entry| entry.1.priority)
                    .map(|entry| entry.0);
                let expected = index.map(|index| model.swap_remove(index));
                assert_eq!(
                    map.pop(),
                    expected.map(|entry| (entry.key, entry.priority, entry.value))
                );
            }
            _ => assert_eq!(
                map.get(&key),
                model
                    .iter()
                    .find_map(|entry| (entry.key == key).then_some(&entry.value))
            ),
        }
        assert_eq!(map.len(), model.len());
        assert_eq!(
            map.peek().map(|entry| entry.1),
            model.iter().map(|entry| &entry.priority).min()
        );
    }
}

#[test]
fn packs_small_planes() {
    let plan = (priority::Plan::<u32, u32, u32>::new(3)).expect("three small entries must fit");
    let (built, cold) = Tracker::measure(|| priority::Min::try_new(plan, Seed(7)));
    assert_eq!(cold, [1, 176, 0, 0]);
    let freed = {
        let mut map = (built).expect("the measured priority map must allocate");
        let ((), hot) = Tracker::measure(|| {
            for key in 1..=3 {
                let priority::Entry::Vacant(entry) = map.try_entry(key).expect("capacity") else {
                    panic!("occupied");
                };
                entry.insert(4 - key, key * 10);
            }
            assert_eq!(map.pop(), Some((3, 1, 30)));
            assert_eq!(map.remove(&1), Some(10));
            assert_eq!(map.pop(), Some((2, 2, 20)));
            assert!(map.is_empty());
        });
        assert_eq!(hot, [0; 4]);
        Tracker::begin()
    };
    assert_eq!(
        freed.finish(),
        common::alloc::system::Activity {
            allocations: 0,
            allocated_bytes: 0,
            deallocations: 1,
            deallocated_bytes: 176
        }
    );
}
