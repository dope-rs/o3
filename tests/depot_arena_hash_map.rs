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
        hash::{
            self,
            runtime::{self, entry},
        },
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
    let mut store = (runtime::Registry::<Borrowed<'_>>::try_new(plan))
        .expect("the four-slot borrowed registry must allocate");
    let key = store
        .insert(Borrowed(&value))
        .ok()
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
                    .try_insert(value & 7, value, |stored, input| stored == input)
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
fn runtime_keeps_shape() {
    assert_eq!(
        std::mem::size_of::<runtime::Registry<[u64; 1], Tag>>(),
        std::mem::size_of::<runtime::Map<[u64; 1], Tag>>()
    );
    let plan = (hash::Plan::new(4)).expect("the fixture capacity must fit");
    let (store, alloc) = Tracker::measure(|| runtime::Registry::<[u64; 1], Tag>::try_new(plan));
    let mut store = (store).expect("the four-resident tagged registry must allocate");
    let bytes = alloc[1];
    assert!(bytes >= 4 * std::mem::size_of::<u64>());
    assert_eq!((alloc[0], alloc[2], alloc[3]), (1, 0, 0));
    let ((), hot) = Tracker::measure(|| {
        let key = (store.insert([7])).expect("the empty registry must admit its resident");
        let entry = store
            .entry(key)
            .expect("the newly inserted resident is live");
        assert!(matches!(entry, entry::Entry::Staged(_)));
        if let entry::Entry::Staged(entry) = entry {
            let indexed = entry.try_reserve(15, &[7][..]).map(map::Vacancy::commit);
            assert!(matches!(indexed, Ok(ref entry) if entry.key() == key));
        }
        let value = (store.get(key)).expect("the inserted runtime resident must remain live");
        let ptr = ptr::from_ref(value);
        let found =
            (store.find(15, |value| *value == [7])).expect("the indexed resident must be found");
        assert_eq!(found.key().index(), key.index());
        assert!((found.key().generation()) == (key.generation()));
        assert_eq!(store.get(key).map(ptr::from_ref), Some(ptr));
        let indexed =
            (store.indexed(key)).expect("the live indexed key must retain its capability");
        assert_eq!(indexed.unindex().remove(), [7]);
    });
    assert_eq!(hot, [0; 4]);
    let ((), freed) = Tracker::measure(move || assert!(store.is_empty()));
    assert_eq!(freed, [0, 0, 1, bytes]);
}

#[test]
fn values_stay_stable() {
    let plan = (hash::Plan::new(8)).expect("the fixture capacity must fit");
    let mut map = (runtime::Map::<u64>::try_new(plan))
        .expect("the eight-resident stable-address map must allocate");
    let mut live = Vec::new();
    for value in 0..8_u64 {
        let key = map
            .try_insert(15, value, |stored, input| stored == input)
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
fn staged_repairs_shift() {
    let plan = (hash::Plan::new(8)).expect("the fixture capacity must fit");
    let mut store = (runtime::Registry::<[u32; 1]>::try_new(plan))
        .expect("the eight-resident staged registry must allocate");
    let mut keys = Vec::new();
    for value in 0..8_u32 {
        let key = (store.insert([value])).expect("the staged registry must admit each model value");
        keys.push(key);
    }
    for (value, key) in keys.iter().copied().enumerate() {
        let entry = store.entry(key).expect("the inserted resident is live");
        assert!(matches!(entry, entry::Entry::Staged(_)));
        if let entry::Entry::Staged(staged) = entry {
            assert!(
                staged
                    .try_reserve(15, &[value as u32][..])
                    .map(map::Vacancy::commit)
                    .is_ok()
            );
        }
    }
    for gone in [3, 0, 7, 1] {
        let indexed =
            (store.indexed(keys[gone])).expect("the selected resident must remain indexed");
        assert!((indexed.unindex().key()) == (keys[gone]));
        assert!(matches!(
            store.entry(keys[gone]),
            Some(entry::Entry::Staged(_))
        ));
    }
    for key in keys {
        assert!(store.entry(key).map(|entry| entry.remove()).is_some());
    }
    assert!(store.is_empty());
}

#[test]
fn staged_index_is_free() {
    let plan = (hash::Plan::new(2)).expect("the fixture capacity must fit");
    let mut store = (runtime::Registry::<[u32; 1]>::try_new(plan))
        .expect("the two-resident staged registry must allocate");
    let left = (store.insert([1])).expect("the left resident must fit the staged registry");
    let right = (store.insert([2])).expect("the right resident must fit the staged registry");
    let &prior = (store.get(left)).expect("the left resident must remain live");
    let ((), hot) = Tracker::measure(|| {
        {
            let entry = store.entry(left).expect("the inserted resident is live");
            assert!(matches!(entry, entry::Entry::Staged(_)));
            if let entry::Entry::Staged(staged) = entry {
                let indexed = staged.try_reserve(7, &[1][..]).map(map::Vacancy::commit);
                assert!(matches!(indexed, Ok(ref entry) if entry.key() == left));
            }
        }

        let entry = store.entry(right).expect("the inserted resident is live");
        assert!(matches!(entry, entry::Entry::Staged(_)));
        if let entry::Entry::Staged(staged) = entry {
            let (mut staged, owner) = staged
                .try_reserve(7, prior.as_slice())
                .err()
                .expect("a matching directory key must collide");
            assert!(owner == left);
            assert!(staged.key() == right);
            *staged.get_mut() = [3];
            let wanted = *staged.get();
            let indexed = staged
                .try_reserve(7, wanted.as_slice())
                .map(map::Vacancy::commit);
            assert!(indexed.is_ok());
            if let Ok(indexed) = indexed {
                let staged = indexed.unindex();
                assert!(staged.key() == right);
                *staged.into_mut() = [4];
            }
        }
        assert_eq!(store.entry(right).map(|entry| entry.remove()), Some([4]));

        let found = (store.find(7, |value| *value == prior))
            .expect("the left resident must remain indexed");
        assert!((found.key()) == (left));
        *found.into_mut() = [5];
        let entry = (store.entry_at(left.index() as usize))
            .expect("the left resident slot must remain occupied");
        assert_eq!(entry.remove(), [5]);
    });
    assert_eq!(hot, [0; 4]);
    assert!(store.is_empty());
}

#[test]
fn fullness_tracks_reuse() {
    for capacity in [0, 1, 4] {
        let plan = (hash::Plan::new(capacity)).expect("fixture capacity must fit");
        let mut store =
            (runtime::Registry::<usize>::try_new(plan)).expect("registry must allocate");
        let ((), hot) = Tracker::measure(|| {
            assert_eq!(store.is_full(), capacity == 0);
            let mut first = None;
            for value in 0..capacity {
                let key = (store.insert(value)).expect("remaining slot must admit its value");
                if value == 0 {
                    first = Some(key);
                }
                assert_eq!(store.is_full(), value + 1 == capacity);
            }
            assert!(matches!(store.insert(99), Err(99)));
            assert!(store.is_full());
            if let Some(first) = first {
                let entry = (store.entry(first)).expect("first generation must remain live");
                assert_eq!(entry.remove(), 0);
                assert!(!store.is_full());
                let next = (store.insert(7)).expect("retired slot must be reusable");
                assert_eq!(next.index(), first.index());
                assert!((next.generation()) != (first.generation()));
                assert!(store.get(first).is_none());
                assert_eq!(store.get(next), Some(&7));
                assert!(store.is_full());
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
        .try_insert(15, 7, |stored, input| stored == input)
        .expect("first resident");
    store
        .try_insert(15, 9, |stored, input| stored == input)
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
        .try_insert(3, 17, |stored, input| stored == input)
        .expect("the dropped vacancy must leave its slot available");
    assert_eq!(key.index(), index.raw());
}

#[test]
fn runtime_keys_are_exact() {
    let plan = (hash::Plan::new(1)).expect("the fixture capacity must fit");
    let mut store = (runtime::Registry::<u64, Tag>::try_new(plan))
        .expect("the one-resident tagged registry must allocate");
    let mut heap = (keyed::Min::<key::Handle<runtime::Domain<Tag>>, u64>::try_new(Count::new(1)))
        .expect("heap storage must fit its declared domain");

    let first = (store.insert(7)).expect("the empty registry must admit its first value");
    let entry = heap.entry(first);
    assert!(matches!(entry, Ok(keyed::Entry::Vacant(_))));
    if let Ok(keyed::Entry::Vacant(entry)) = entry {
        entry.insert(11);
    }
    assert_eq!(store.entry(first).map(|entry| entry.remove()), Some(7));

    let next = (store.insert(9)).expect("the removed resident slot must be reusable");
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
