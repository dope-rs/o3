pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{array, cell, collections::BTreeMap, ptr, rc::Rc};

use common::{
    alloc::system::Tracker,
    depot::{
        arena::{Item, Member, Slot},
        model::{Random, drops::Order},
    },
};
use o3::depot::{
    self,
    arena::{
        hash::{
            self, errors,
            runtime::{self, entry},
        },
        slab::key,
    },
    index::Access as _,
    queue::links,
    raw::Key as _,
};

#[test]
fn drops_live_reverse_dense() {
    let drops = cell::Cell::new(Some(0));
    let mut map =
        runtime::Map::<Order<'_>>::try_new(hash::Plan::new(4).expect("capacity")).expect("map");
    let keys: [_; 4] = array::from_fn(|index| {
        map.try_insert(0, Order(&drops, index as u64 + 1), |_, _| false)
            .expect("vacancy")
    });
    drop(map.remove(keys[1]).expect("live"));
    drop(map);
    assert_eq!(drops.get(), Some(2341));
}

#[test]
fn reserve_stays_private() {
    let plan = hash::Plan::new(2).expect("registry capacity");
    let mut store = runtime::Registry::<[u32; 1]>::try_new(plan).expect("registry");
    let key = store.insert([1]).expect("staged resident");
    let address = ptr::from_ref(store.get(key).expect("resident"));
    let ((), counts) = Tracker::measure(|| {
        {
            let staged = match store.entry(key).expect("live resident") {
                entry::Entry::Staged(staged) => Some(staged),
                entry::Entry::Indexed(_) => None,
            }
            .expect("unindexed resident");
            let reserved = staged
                .try_reserve(9, &[3][..])
                .map_err(|(_, key)| (key.index(), key.generation().get()))
                .expect("unindexed resident must reserve the directory key");
            assert_eq!(reserved.get(), &[1]);
        }
        assert!(store.indexed(key).is_none());
        assert!(store.find(9, |value| *value == [3]).is_none());
        assert_eq!(store.get(key).map(ptr::from_ref), Some(address));
        let staged = match store.entry(key).expect("resident after abandonment") {
            entry::Entry::Staged(staged) => Some(staged),
            entry::Entry::Indexed(_) => None,
        }
        .expect("abandonment leaves the slot unindexed");
        let mut reserved = staged
            .try_reserve(9, &[3][..])
            .map_err(|(_, key)| (key.index(), key.generation().get()))
            .expect("abandoned reservation must release the directory key");
        *reserved.get_mut() = [3];
        assert!(reserved.commit().key() == key);
        assert!(
            store
                .find(9, |value| *value == [3])
                .expect("committed key")
                .key()
                == key
        );
        assert_eq!(store.get(key).map(ptr::from_ref), Some(address));
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn insertion_reports_owner() {
    let plan = (hash::Plan::new(2)).expect("the fixture capacity must fit");
    let mut map =
        (runtime::Map::<u32>::try_new(plan)).expect("the two-resident collision map must allocate");
    let compared = cell::Cell::new(false);
    let first = map
        .try_insert(9, 1_u32, |_, _| {
            compared.set(true);
            false
        })
        .expect("initial vacancy");
    assert!(!compared.get());
    let prior = *map.get(first).expect("first resident remains live");
    let error = map
        .try_insert(9, 2, |stored, input| *stored == prior && *input == 2)
        .expect_err("matching resident");
    assert!(matches!(error, hash::errors::Insert::Occupied { key, value: 2 } if key == first));
    assert_eq!(map.len(), 1);
}

#[test]
fn matcher_keeps_failure() {
    let plan = hash::Plan::new(2).expect("capacity");
    let mut map = runtime::Map::<u32>::try_new(plan).expect("map");
    let key = map.try_insert(9, 7, |_, _| false).expect("resident");
    let life = Rc::new(());
    let token = String::from("matcher");
    let mark = &token;
    let held = Rc::clone(&life);
    let error = map.try_key(9, move |value| {
        assert_eq!(Rc::strong_count(&held), 2);
        Err::<bool, _>((*value, mark))
    });
    let error = error.expect_err("matcher failure");
    assert_eq!(error.0, 7);
    assert!(ptr::eq(error.1, mark));
    assert_eq!(Rc::strong_count(&life), 1);

    let held = Rc::clone(&life);
    let error = map.try_entry(9, move |value| {
        assert_eq!(Rc::strong_count(&held), 2);
        Err::<bool, _>((*value, mark))
    });
    let error = error.err().expect("entry matcher failure");
    assert_eq!(error.0, 7);
    assert!(ptr::eq(error.1, mark));
    assert_eq!(Rc::strong_count(&life), 1);
    assert_eq!(map.len(), 1);
    assert_eq!(map.get(key), Some(&7));

    for value in [7, 8] {
        let held = Rc::clone(&life);
        let found = map
            .try_key(9, move |stored| {
                assert_eq!(Rc::strong_count(&held), 2);
                Ok::<_, ()>(*stored == value)
            })
            .expect("lookup");
        assert!(found == (value == 7).then_some(key));
        assert_eq!(Rc::strong_count(&life), 1);
    }

    let held = Rc::clone(&life);
    let created = map
        .try_create(
            9,
            move |_| {
                assert_eq!(Rc::strong_count(&held), 2);
                false
            },
            |_| {
                assert_eq!(Rc::strong_count(&life), 1);
                8
            },
        )
        .expect("created");
    assert_eq!(map.get(created), Some(&8));
}

#[test]
fn rejects_unproved_keys() {
    let plan = (hash::Plan::new(1)).expect("capacity must fit");
    let mut map = (runtime::Map::<u64>::try_new(plan)).expect("map must allocate");
    let first = map
        .try_insert(3, 3, |stored, input| stored == input)
        .expect("hash key must own its original vacant slot");

    let outside = (key::Handle::<runtime::Domain>::decode(first.into_raw() | 7))
        .expect("generation must remain nonzero");
    assert!(
        matches!(entry::Live::<_, (), entry::Occupied>::try_new(&mut map, outside), Err(errors::Lookup::Index { key, capacity: 1 }) if key == outside)
    );
    let selected = (entry::Live::<_, (), entry::Occupied>::try_new(&mut map, first))
        .expect("hash key must retain its original generation");

    assert_eq!(selected.remove(), 3);
    assert!(map.get(first).is_none());
    assert!(
        matches!(entry::Live::<_, (), entry::Occupied>::try_new(&mut map, first), Err(errors::Lookup::Vacant { key, actual }) if key == first && Some(actual) == first.generation().checked_add(1))
    );
    let second = (map.try_create(5, |value| *value == 5, |_| 5))
        .expect("hash key must own its original vacant slot");

    assert_eq!(second.index(), first.index());
    assert!(
        matches!(entry::Live::<_, (), entry::Occupied>::try_new(&mut map, first), Err(errors::Lookup::Generation { key, actual }) if key == first && actual == second.generation())
    );
    let (selected, delta) =
        Tracker::measure(|| entry::Live::<_, (), entry::Occupied>::try_new(&mut map, second));
    assert_eq!(delta, [0; 4]);
    let mut selected = (selected).expect("hash key must retain its original generation");

    *selected.get_mut() += 2;
    assert_eq!(*selected.get(), 7);
    selected.discard();
    assert_eq!(map.len(), 0);
    assert!((map.key(5, |_| true)).is_none());
}

#[test]
fn rejects_before_create() {
    let plan = (hash::Plan::new(1)).expect("capacity must fit");
    let mut map = (runtime::Map::<u64>::try_new(plan)).expect("map must allocate");
    let mut made = 0;
    let mut live = (map.try_live(
        3,
        |value| *value == 3,
        |index| {
            made += 1;
            assert_eq!(index.raw(), 0);
            3
        },
    ))
    .expect("hash key must own its original vacant slot");
    *live.get_mut() += 4;
    assert_eq!(*live.get(), 7);
    *live.get_mut() = 3;
    let first = live.key();

    let (clash, delta) = Tracker::measure(|| {
        map.try_live(
            3,
            |value| *value == 3,
            |_| {
                made += 1;
                7
            },
        )
    });
    assert!(matches!(clash, Err(errors::Create::Occupied { key }) if key == first));
    assert_eq!(delta, [0; 4]);
    let (full, delta) = Tracker::measure(|| {
        map.try_live(
            5,
            |value| *value == 5,
            |_| {
                made += 1;
                5
            },
        )
    });
    assert!(matches!(full, Err(errors::Create::Full { capacity: 1 })));
    assert_eq!(delta, [0; 4]);
    assert_eq!(made, 1);
    assert_eq!(map.len(), 1);
    assert_eq!(map.get(first), Some(&3));
    assert!((map.key(3, |value| *value == 3)) == (Some(first)));
}

#[test]
fn randomized_model_matches() {
    use hash::errors::Insert;

    const CAPACITY: usize = 17;
    const DOMAIN: u8 = 31;
    let plan = (hash::Plan::new(CAPACITY)).expect("the fixture capacity must fit");
    let mut map = (runtime::Map::<Item>::try_new(plan))
        .expect("the seventeen-resident model map must allocate");
    let mut model = BTreeMap::new();
    let mut keys = [None; DOMAIN as usize];
    let mut ptrs = [None; DOMAIN as usize];
    let mut stale = Vec::new();
    let mut rng = Random::<u64, 13, 7, 17>::try_new(0x6a09_e667_f3bc_c909_u64).expect("shifts");

    for step in 0..100_000_u64 {
        let random = rng.advance();
        let op = random % 5;
        let random = rng.advance();
        let id = (random % u64::from(DOMAIN)) as u8;
        match op {
            0 | 1 => {
                let item = Item { id, revision: step };
                let inserted =
                    map.try_insert(Item::hash(id), item, |stored, input| stored.id == input.id);
                if let Some(prior) = model.get(&id) {
                    let error = inserted.expect_err("model duplicate");
                    assert!(matches!(error, Insert::Occupied { .. }));
                    if let Insert::Occupied { key, value } = error {
                        assert!(key == keys[id as usize].expect("model resident key"));
                        assert_eq!(value.revision, step);
                        assert_eq!(map.get(key).expect("model resident").revision, *prior);
                    }
                } else if model.len() == CAPACITY {
                    let error = inserted.expect_err("full model map");
                    assert!(
                        matches!(error, Insert::Full(value) if (value.id, value.revision) == (id, step))
                    );
                } else {
                    assert!(inserted.is_ok());
                    if let Ok(key) = inserted {
                        model.insert(id, step);
                        keys[id as usize] = Some(key);
                        ptrs[id as usize] =
                            Some(map.get(key).expect("inserted model resident") as *const Item);
                    }
                }
            }
            2 => {
                if let Some(key) = keys[id as usize].take() {
                    let gone = map.remove(key).expect("model resident removal");
                    assert_eq!(model.remove(&id), Some(gone.revision));
                    ptrs[id as usize] = None;
                    stale.push(key);
                }
            }
            3 => {
                assert!((map.key(Item::hash(id), |stored| stored.id == id)) == (keys[id as usize]))
            }
            _ => {
                if let Some(key) = keys[id as usize] {
                    map.get_mut(key).expect("mutable model resident").revision = step;
                    model.insert(id, step);
                }
            }
        }
        assert_eq!(map.len(), model.len());
        for (&id, &revision) in &model {
            let key = keys[id as usize].expect("live model key");
            let item = map.get(key).expect("live model resident");
            assert_eq!((item.id, item.revision), (id, revision));
            assert_eq!(
                item as *const Item,
                ptrs[id as usize].expect("live resident address")
            );
        }
        for key in stale.iter().rev().take(8) {
            assert!(map.get(*key).is_none());
        }
    }
}

#[test]
fn retains_owner_context() {
    let plan = (hash::Plan::new(1)).expect("capacity must fit");
    let mut map = (runtime::Map::<u64>::try_new(plan)).expect("map must allocate");
    let key = (map.try_create(11, |value| *value == 11, |_| 11))
        .expect("hash key must own its original vacant slot");

    let mut context = 17;
    let ((observed, value), delta) = Tracker::measure(|| {
        let live = unsafe { Slot::new(&mut map, key) }.into_live();
        // SAFETY: this preserves the retained Map, exact slot, and indexed plane.
        let live = unsafe {
            live.map(|(slot, key)| {
                (
                    (slot.into_owner(), &mut context),
                    <links::Index as depot::raw::Word>::from_offset(key.index() as usize),
                )
            })
        };
        let (selected, context) = entry::Live::<_, (), entry::Occupied>::from_live(live);
        let mut member = Member(selected);
        let live = member.read();
        // SAFETY: the guard is the same original owner of this initialized slot.
        let live = unsafe {
            live.map(|(member, key)| {
                (
                    &member.0,
                    <links::Index as depot::raw::Word>::from_offset(key.index() as usize),
                )
            })
        };
        let (observed, value) = entry::Live::<_, (), entry::Occupied>::project(live);
        assert_eq!(*value, 11);
        *member.0.get_mut() += 12;
        *context += 1;
        (observed, member.0.remove())
    });
    assert!((observed, value, context) == (key, 23, 18));
    assert_eq!(delta, [0; 4]);
    assert_eq!(map.len(), 0);
    assert_eq!(map.get(key), None);
    assert!((map.key(11, |_| true)).is_none());
}

#[test]
fn compares_owned_input() {
    let plan = (hash::Plan::new(2)).expect("the fixture capacity must fit");
    let mut map =
        (runtime::Map::<String, ()>::try_new(plan)).expect("a two-slot hash map must allocate");
    let first = map
        .try_insert(7, String::from("same"), |stored, input| stored == input)
        .expect("the first owned hash input must fit");
    let error = map
        .try_insert(7, String::from("same"), |stored, input| stored == input)
        .expect_err("equal key");
    assert!(matches!(
        error,
        hash::errors::Insert::Occupied { key, value } if key == first && value == "same"
    ));
    assert!((map.key_at(first.index() as usize)) == (Some(first)));
}
