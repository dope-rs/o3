pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell, ptr};

use common::{
    alloc::system::Tracker,
    depot::arena::{Borrowed, Failure, Tag},
};
use o3::depot::{
    arena::{
        hash::{self, errors, runtime},
        slab::key,
    },
    capacity::Count,
    heap::keyed,
    map,
};

#[test]
fn borrows_keep_shape() {
    let value = 7;
    let plan = (hash::Plan::new(4)).expect("the fixture capacity must fit");
    let mut store = runtime::Map::<Borrowed<'_>>::try_new(plan).expect("borrowed map");
    let key = store
        .try_create(7, |_| false, |_| Borrowed(&value))
        .expect("borrowed resident");
    assert_eq!(store.get(key).map(|stored| *stored.0), Some(value));
}

#[test]
fn bounds_fit_u32() {
    if usize::BITS != 64 {
        return;
    }
    let limit = 1_usize << 30;
    let excess = limit + 1;

    hash::Plan::new(limit)
        .as_ref()
        .expect("required fixture outcome");
    let map = map::Plan::new(limit).expect("the shared index limit must fit");
    let (hash, counts) = Tracker::measure(|| hash::Plan::try_from(map));
    assert!((hash.expect("the resident layout must fit").capacity()) == (map.capacity()));
    assert_eq!(counts, [0; 4]);
    let expected = map::Plan::new(excess).err().expect("domain rejection");
    assert_eq!(expected.requested(), excess);
    assert!(
        matches!(hash::Plan::new(excess), Err(map::Capacity::Domain(error)) if error == expected)
    );
}

#[test]
fn allocations_are_exact() {
    let shape = map::Plan::fixed::<64>();
    let plan = hash::Plan::try_from(shape).expect("the shared plan must fit the resident layout");
    let ((), counts) = Tracker::measure(|| {
        let mut map =
            runtime::Map::<u64>::try_new(plan).expect("the sixty-four-resident map must allocate");
        let ((), hot) = Tracker::measure(|| {
            let mut keys = [None; 64];
            for value in 0..64_u64 {
                let key = map
                    .try_create(value & 7, |stored| *stored == value, |_| value)
                    .expect("remaining resident slot");
                keys[value as usize] = Some(key);
            }
            for value in 0..64_u64 {
                let key =
                    (keys[value as usize]).expect("every inserted resident must retain its key");
                assert_eq!(map.get(key), Some(&value));
                assert!((map.key(value & 7, |stored| *stored == value)) == (Some(key)));
            }
            for key in keys.into_iter().flatten() {
                assert!(map.remove(key).is_some());
            }
        });
        assert_eq!(hot, [0; 4]);
    });
    assert_eq!((counts[0], counts[2]), (1, 1));
    assert!(counts[1] >= 64 * std::mem::size_of::<u64>());
    assert_eq!(counts[1], counts[3]);
}

#[test]
fn values_stay_stable() {
    let plan = (hash::Plan::new(8)).expect("the fixture capacity must fit");
    let mut map = (runtime::Map::<u64>::try_new(plan))
        .expect("the eight-resident stable-address map must allocate");
    let mut live = Vec::new();
    for value in 0..8_u64 {
        let key = map
            .try_create(15, |stored| *stored == value, |_| value)
            .expect("remaining resident slot");
        let stored = map
            .get(key)
            .expect("the inserted resident must remain live");
        live.push((key, ptr::from_ref(stored)));
    }
    let gone = live.remove(3);
    assert_eq!(map.remove(gone.0), Some(3));
    for (key, ptr) in &live {
        assert_eq!(map.get(*key).map(ptr::from_ref), Some(*ptr));
    }
    assert_eq!(map.values().count(), 7);
}

#[test]
fn fullness_tracks_reuse() {
    for capacity in [0, 1, 4] {
        let plan = (hash::Plan::new(capacity)).expect("fixture capacity must fit");
        let mut store = runtime::Map::<usize>::try_new(plan).expect("map");
        let ((), hot) = Tracker::measure(|| {
            let mut first = None;
            for value in 0..capacity {
                let key = store
                    .try_create(value as u64, |stored| *stored == value, |_| value)
                    .expect("remaining slot");
                if value == 0 {
                    first = Some(key);
                }
            }
            assert!(matches!(store.try_create(99, |_| false, |_| 99),
                Err(errors::Create::Full { capacity: actual }) if actual == capacity));
            if let Some(first) = first {
                assert_eq!(store.remove(first), Some(0));
                let next = store
                    .try_create(7, |stored| *stored == 7, |_| 7)
                    .expect("reusable slot");
                assert_eq!(next.index(), first.index());
                assert!((next.generation()) != (first.generation()));
                assert!(store.get(first).is_none());
                assert_eq!(store.get(next), Some(&7));
                assert_eq!(store.len(), capacity);
            }
        });
        assert_eq!(hot, [0; 4]);
    }
}

#[test]
fn stops_before_mutation() {
    let plan = hash::Plan::new(3).expect("three-resident plan");
    let mut store = runtime::Map::<u64>::try_new(plan).expect("allocate resident map");
    let first = store
        .try_create(15, |stored| *stored == 7, |_| 7)
        .expect("first resident");
    store
        .try_create(15, |stored| *stored == 9, |_| 9)
        .expect("second resident");
    let ((), counts) = Tracker::measure(|| {
        let calls = cell::Cell::new(0);
        let error = store
            .try_key(15, |stored| {
                calls.set(calls.get() + 1);
                Err(Failure {
                    code: 31,
                    observed: *stored,
                })
            })
            .expect_err("comparison must fail");
        assert_eq!(
            error,
            Failure {
                code: 31,
                observed: 7
            }
        );
        assert_eq!(calls.get(), 1);
        let error = store
            .try_entry(15, |stored| {
                Err(Failure {
                    code: 41,
                    observed: *stored,
                })
            })
            .err()
            .expect("entry comparison must fail");
        assert_eq!(
            error,
            Failure {
                code: 41,
                observed: 7
            }
        );
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(first), Some(&7));
        let slot = match store
            .try_entry(15, |stored| Ok::<_, Failure>(*stored == 11))
            .expect("successful comparison")
        {
            map::Entry::Vacant(slot) => slot,
            map::Entry::Occupied(_) => None,
        }
        .expect("remaining resident slot");
        let key = slot.insert(11);
        assert_eq!(store.get(key), Some(&11));
        assert_eq!(store.len(), 3);
        assert!(matches!(
            store.try_entry(15, |stored| Ok::<_, Failure>(*stored == 13)),
            Ok(map::Entry::Vacant(None))
        ));
        assert!(
            matches!(store.try_entry(15, |stored| Ok::<_, Failure>(*stored == 11)), Ok(map::Entry::Occupied(entry)) if entry.key() == key)
        );
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn preserves_spare() {
    let plan = hash::Plan::new(1).expect("one-resident plan");
    let mut store = runtime::Map::<u64>::try_new(plan).expect("allocate resident map");
    let index = match store.try_entry(3, |stored| Ok::<_, Failure>(*stored == 17)) {
        Ok(map::Entry::Vacant(Some(slot))) => Some(slot.index()),
        _ => None,
    }
    .expect("the empty map must reserve a slot");
    assert_eq!(store.len(), 0);
    let key = store
        .try_create(3, |stored| *stored == 17, |_| 17)
        .expect("the dropped vacancy must leave its slot available");
    assert_eq!(key.index(), index.raw());
}

#[test]
fn runtime_keys_are_exact() {
    let plan = (hash::Plan::new(1)).expect("the fixture capacity must fit");
    let mut store = runtime::Map::<u64, Tag>::try_new(plan).expect("tagged map");
    let mut heap = (keyed::Min::<key::Handle<runtime::Domain<Tag>>, u64>::try_new(Count::new(1)))
        .expect("heap storage must fit its declared domain");

    let diagnostic = |cause| match cause {
        errors::Create::Full { capacity } => (capacity, None),
        errors::Create::Occupied { key } => (0, Some((key.index(), key.generation()))),
    };
    let first = store
        .try_create(7, |stored| *stored == 7, |_| 7)
        .map_err(diagnostic)
        .expect("first resident");
    let entry = heap.entry(first);
    assert!(matches!(entry, Ok(keyed::Entry::Vacant(_))));
    if let Ok(keyed::Entry::Vacant(entry)) = entry {
        entry.insert(11);
    }
    assert_eq!(store.remove(first), Some(7));

    let next = store
        .try_create(9, |stored| *stored == 9, |_| 9)
        .map_err(diagnostic)
        .expect("reusable slot");
    assert_eq!(next.index(), first.index());
    assert!((next.generation()) != (first.generation()));
    let entry = heap.entry(next);
    assert!(matches!(entry, Ok(keyed::Entry::Conflict(ref clash))
        if clash.current() == first && clash.requested() == next));
    let entry = (heap.first_entry()).expect("the first generation must remain in the heap");
    let popped = entry.key();
    let value = entry.remove();
    assert_eq!(popped.index(), first.index());
    assert!((popped.generation()) == (first.generation()));
    assert_eq!(value, 11);
}
