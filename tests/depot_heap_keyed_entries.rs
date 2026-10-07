pub mod common;

use common::{
    alloc::system::{Activity, Tracker},
    depot::{index::key::Id, model::Lcg},
};
use o3::depot::{
    capacity::Count,
    heap::{
        self,
        keyed::{self, Entry},
    },
    raw::Key,
};

const COMPACT_CAPACITY_257_BYTES: usize = 4_632;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn hot_path_allocations() {
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<keyed::Min<Id<'static>, u64, u16>>(), 24);
    }
    let bytes = COMPACT_CAPACITY_257_BYTES;
    let allocs = {
        let (heap, allocs) =
            Tracker::measure(|| keyed::Min::<Id<'static>, u64, u16>::try_new(Count::new(257)));
        let mut heap = heap.expect("the heap owner must be available");

        assert_eq!(allocs, [1, bytes, 0, 0]);

        let ((), allocs) = Tracker::measure(|| {
            for index in 0..257 {
                let entry = heap.entry(Id::try_new(index, 1).expect("identity components fit"));
                let entry = match entry {
                    Ok(Entry::Vacant(entry)) => Some(entry),
                    _ => None,
                }
                .expect("each in-bounds compact identity must start vacant");
                entry.insert((index * 7) as u64);
            }
            for index in 0..257 {
                let key = Id::try_new(index, 1).expect("identity components fit");
                let entry = heap.entry(key);
                let entry = match entry {
                    Ok(Entry::Occupied(entry)) => Some(entry),
                    _ => None,
                }
                .expect("each inserted exact identity must be occupied");
                entry.set(index as u64);
            }
            while heap
                .first_entry()
                .map(|entry| (entry.key(), entry.remove()))
                .is_some()
            {}
        });
        assert_eq!(allocs, [0; 4]);

        Tracker::begin()
    };
    let allocs = allocs.finish();
    assert_eq!(
        allocs,
        Activity {
            deallocations: 1,
            deallocated_bytes: bytes,
            ..Activity::default()
        }
    );
}

#[test]
fn minimum_change_is_exact() {
    let mut heap =
        keyed::Min::<Id<'static>, u64, u16>::try_new(Count::new(4)).expect("four heap slots");
    let first = Id::try_new(0, 1).expect("identity components fit");
    let second = Id::try_new(1, 1).expect("identity components fit");
    let third = Id::try_new(2, 1).expect("identity components fit");

    for (key, priority) in [(first, 20), (second, 30), (third, 10)] {
        let Entry::Vacant(entry) = heap.entry(key).expect("bounded key") else {
            panic!("vacant entry");
        };
        entry.insert(priority);
    }
    let heap::Probe::Occupied(entry) = heap.probe(second) else {
        panic!("occupied entry");
    };
    entry.set(5);
    for (key, change) in [
        (first, heap::Change::Contents),
        (second, heap::Change::Minimum),
    ] {
        let heap::Probe::Occupied(entry) = heap.probe(key) else {
            panic!("occupied entry");
        };
        assert!(entry.retire_min() == change);
    }
}

#[test]
fn preserves_generation() {
    let mut heap = keyed::Min::<Id<'static>, u64, u16>::try_new(Count::new(1))
        .expect("one physical heap slot");
    let old = Id::try_new(0, 7).expect("identity components fit");
    let next = Id::try_new(0, 8).expect("identity components fit");
    let outside = Id::try_new(1, 7).expect("identity components fit");
    assert!(matches!(heap.probe(next), heap::Probe::Vacant));
    assert!(matches!(heap.entry(old), Ok(Entry::Vacant(_))));
    assert!(heap.is_empty());
    let Entry::Vacant(entry) = heap.entry(old).expect("bounded old generation") else {
        panic!("vacant entry");
    };
    entry.insert(40);
    assert_eq!(heap.peek(), Some((old, &40)));
    assert!(matches!(heap.probe(outside), heap::Probe::OutOfRange));
    let Entry::Conflict(conflict) = heap.entry(next).expect("bounded next generation") else {
        panic!("conflicting generation");
    };
    assert_eq!(conflict.requested(), next);
    assert_eq!(conflict.current(), old);
    assert!(matches!(heap.probe(next), heap::Probe::Conflict));
    heap.retire(next);
    heap.retire(outside);
    assert_eq!(heap.peek(), Some((old, &40)));
    let heap::Probe::Occupied(entry) = heap.probe(old) else {
        panic!("occupied generation");
    };
    assert_eq!(*entry.priority(), 40);
    assert_eq!(entry.remove(), 40);
    assert!(matches!(heap.probe(next), heap::Probe::Vacant));
    let Entry::Vacant(entry) = heap.entry(next).expect("bounded next generation") else {
        panic!("vacant entry");
    };
    entry.insert(3);
    assert!(matches!(heap.probe(old), heap::Probe::Conflict));
    assert!(heap.peek() == Some((next, &3)));
    heap.retire(next);
    assert!(matches!(heap.probe(old), heap::Probe::Vacant));
    assert!(heap.is_empty());
}

#[test]
fn priority_index_order() {
    let heap = keyed::Min::<Id<'static>, u64, u16>::try_new(Count::new(8));
    let mut heap = heap.expect("the heap owner must be available");

    for (index, priority) in [(7, 9), (5, 3), (2, 3), (0, 3), (6, 1)] {
        let entry = heap.entry(Id::try_new(index, 1).expect("identity components fit"));
        let entry = match entry {
            Ok(Entry::Vacant(entry)) => Some(entry),
            _ => None,
        }
        .expect("each unique priority key must start vacant");
        entry.insert(priority);
    }
    for (index, priority) in [(6, 1), (0, 3), (2, 3), (5, 3), (7, 9)] {
        assert!(
            heap.first_entry()
                .map(|entry| (entry.key(), entry.remove()))
                == Some((
                    Id::try_new(index, 1).expect("identity components fit"),
                    priority
                ))
        );
    }
}

#[test]
fn random_churn_model() {
    const CAPACITY: usize = 61;
    const OPERATIONS: usize = 50_000;
    let heap = keyed::Min::<Id<'static>, u64, u16>::try_new(Count::new(CAPACITY as u32));
    let mut heap = heap.expect("the heap owner must be available");

    let mut model = [None; CAPACITY];
    let mut stale = [None; CAPACITY];
    let mut epoch = 0u64;
    let mut rng = Lcg::<1_442_695_040_888_963_407>::new(0x243f_6a88_85a3_08d3u64);

    for _ in 0..OPERATIONS {
        let random = rng.advance();
        let index = random as usize % CAPACITY;
        match random % 7 {
            0 | 1 => {
                epoch += 1;
                let key = Id::try_new(index, epoch).expect("identity components fit");
                let entry = heap.entry(key);
                let entry = entry.expect("the keyed entry must be available");
                match model[index] {
                    None => {
                        let entry = match entry {
                            Entry::Vacant(entry) => Some(entry),
                            _ => None,
                        }
                        .expect("an absent model key must have a vacant entry");
                        let priority = random >> 32;
                        entry.insert(priority);
                        model[index] = Some((key, priority));
                    }
                    Some((current, _)) => {
                        let conflict = match entry {
                            Entry::Conflict(conflict) => Some(conflict),
                            _ => None,
                        }
                        .expect("an occupied model key must conflict with the new generation");
                        assert!(conflict.current() == current);
                    }
                }
            }
            2 => {
                if let Some((key, priority)) = model[index] {
                    let next = priority ^ random.rotate_left(17);
                    let entry = heap.entry(key);
                    let entry = match entry {
                        Ok(Entry::Occupied(entry)) => Some(entry),
                        _ => None,
                    }
                    .expect("a modeled key must remain exactly occupied");
                    assert_eq!(*entry.priority(), priority);
                    entry.set(next);
                    model[index] = Some((key, next));
                }
            }
            3 => {
                if let Some((key, priority)) = model[index].take() {
                    let (entry,) = match heap.probe(key) {
                        heap::Probe::Occupied(entry) => Some((entry,)),
                        _ => None,
                    }
                    .expect("the modeled generation must remain occupied");
                    assert_eq!(entry.remove(), priority);
                    stale[index] = Some(key);
                }
            }
            4 => {
                if let Some(key) = stale[index] {
                    assert!(matches!(
                        heap.probe(key),
                        heap::Probe::Vacant | heap::Probe::Conflict
                    ));
                }
            }
            5 => {
                let expected = model
                    .iter()
                    .flatten()
                    .copied()
                    .min_by_key(|(key, priority)| (*priority, key.index()));
                assert!(heap.peek().map(|(key, value)| (key, *value)) == expected);
            }
            _ => {
                let expected = model
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        entry.map(|(key, priority)| (index, key, priority))
                    })
                    .min_by_key(|item| (item.2, item.1.index()));
                assert!(
                    heap.first_entry()
                        .map(|entry| (entry.key(), entry.remove()))
                        == expected.map(|(_, key, priority)| (key, priority))
                );
                if let Some((index, _, _)) = expected {
                    model[index] = None;
                }
            }
        }
        assert_eq!(heap.len(), model.iter().flatten().count());
        for (key, priority) in model.iter().flatten() {
            assert!(
                matches!(heap.probe(*key), heap::Probe::Occupied(entry) if entry.priority() == priority)
            );
        }
    }
}

#[test]
fn compact_position_limit() {
    let maximum = Count::new(u32::from(u16::MAX));
    assert!(
        keyed::Min::<Id<'static>, (), u16>::try_new(maximum).is_ok(),
        "required fixture outcome"
    );
    let count = Count::new(u32::from(u16::MAX) + 1);
    let (actual, maximum) = match (keyed::Min::<Id<'static>, (), u16>::try_new(count))
        .err()
        .expect("the first count above the position width must retain its exact failure")
    {
        heap::Error::Capacity {
            requested: actual,
            maximum,
        } => Some((actual, maximum)),
        _ => None,
    }
    .expect("the first count above the position width must retain its exact failure");
    assert!((actual) == (count));
    assert_eq!(maximum.get(), usize::from(u16::MAX));
}

#[test]
fn empty_owner() {
    use std::mem;

    assert_eq!(
        mem::size_of::<Option<keyed::Min<Id<'static>, u64, u16>>>(),
        mem::size_of::<keyed::Min<Id<'static>, u64, u16>>()
    );
    let released = {
        let (built, activity) = Tracker::measure(|| {
            Tracker::fail_after(0, || {
                keyed::Min::<Id<'static>, u64, u16>::try_new(Count::EMPTY)
            })
        });
        assert_eq!(activity, [0; 4]);
        let mut heap = built.expect("the heap owner must be available");

        assert_eq!(heap.len(), 0);
        assert!(heap.peek().is_none());
        assert!(
            heap.first_entry()
                .map(|entry| (entry.key(), entry.remove()))
                .is_none()
        );
        Tracker::begin()
    };
    let released = released.finish();
    assert_eq!(released, Activity::default());
}
