pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{
    cell::{self, Cell},
    collections::{BTreeSet, HashMap},
    mem,
};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{
        arena::slab::{Family, external::Epoch},
        model::Lcg,
    },
};
use o3::depot::{
    arena::slab::{
        self, external,
        key::{self, Handle},
        lookup,
    },
    capacity::{self, Count},
};

#[test]
fn rejects_identities() {
    let mut table =
        (slab::Store::<u64, (), 2>::try_new(capacity::Count::new(1))).expect("slab allocation");
    for index in [1, u32::MAX] {
        let missing = key::Handle::from_parts(key::Parts::new(index, key::Generation::MIN));
        assert!(
            matches!(table.try_entry(missing), Err(key::Error::Index { key, capacity }) if key == missing && capacity == capacity::Count::new(1))
        );
    }
    assert!(
        (slab::Slots::new(&mut table)
            .try_entry_at(1)
            .map(|entry| (*entry.get(), entry.key())))
            == (Err(lookup::Error::Index {
                index: 1,
                capacity: capacity::Count::new(1)
            }))
    );
    assert!(
        (slab::Slots::new(&mut table)
            .try_entry_at(0)
            .map(|entry| (*entry.get(), entry.key())))
            == (Err(lookup::Error::Free { index: 0 }))
    );
    let initial = key::Handle::from_parts(key::Parts::new(0, key::Generation::MIN));
    assert!(
        matches!(table.try_entry(initial), Err(key::Error::Free { key, actual }) if key == initial && actual == key::Generation::MIN)
    );
    let first = (table.insert(7)).expect("first insertion");
    assert_eq!(table.remove(first), Some(7));
    assert!(
        matches!(table.try_entry(first), Err(key::Error::Free { key, actual }) if key == first && actual.get() == 2)
    );
    let second = (table.insert(11)).expect("second insertion");
    assert!(
        matches!(table.try_entry(first), Err(key::Error::Generation { key, actual }) if key == first && actual == second.generation())
    );
    assert!(
        (slab::Slots::new(&mut table)
            .try_entry_at(0)
            .map(|entry| (*entry.get(), entry.key())))
            == (Ok((11, second)))
    );
    assert_eq!(table.remove(second), Some(11));
    assert!(
        matches!(table.try_entry(second), Err(key::Error::Retired { key, actual, retired: () }) if key == second && actual == second.generation())
    );
    assert!(
        (slab::Slots::new(&mut table)
            .try_entry_at(0)
            .map(|entry| (*entry.get(), entry.key())))
            == (Err(lookup::Error::Retired {
                index: 0,
                retired: ()
            }))
    );
}

#[test]
fn retains_reservations() {
    let mut table = (slab::Store::<u8>::try_new(capacity::Count::new(1))).expect("slab allocation");
    let reserved = (table.try_vacant()).expect("vacancy");
    let key = key::Handle::from_parts(key::Parts::new(0, key::Generation::MIN));
    mem::forget(reserved);
    assert!(
        matches!(table.try_entry(key), Err(key::Error::Reserved { key: observed, actual }) if observed == key && actual == key.generation())
    );
    assert!(
        (slab::Slots::new(&mut table)
            .try_entry_at(key.index())
            .map(|entry| (*entry.get(), entry.key())))
            == (Err(lookup::Error::Reserved { index: key.index() }))
    );
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn reuses_borrow() {
    let mut table =
        (slab::Store::<u64>::try_new(capacity::Count::new(3))).expect("slab allocation");
    let key = (table.insert(13)).expect("insertion");
    let ((), traffic) = Tracker::measure(|| {
        let value = table.try_get(key).expect("live read");
        assert_eq!(*value + *value, 26);
        let mut entry = table
            .try_entry(key)
            .expect("the occupied slab entry must be available");

        *entry.get_mut() += 7;
        assert_eq!(*entry.get(), 20);
        assert_eq!(entry.remove(), 20);
        assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
    });
    assert_eq!(traffic, [0; 4]);
}

#[test]
fn drops_forgotten() {
    let count = Cell::new(Some(0));
    {
        let table = (slab::Cell::<Counted<&Cell<Option<usize>>>, (), { u32::MAX }, false>::try_new(
            capacity::Count::new(3),
        ))
        .expect("slab allocation");
        let vacancy = (table.vacancy()).expect("vacancy");
        mem::forget(vacancy.stage(Counted(&count)));
        let vacancy = (table.vacancy()).expect("second insertion");
        let key = vacancy.insert(Counted(&count));
        let entry = (table.entry(key.parts())).expect("the published generation is removable");
        entry.discard();
        assert_eq!(count.get(), Some(1));
    }
    assert_eq!(count.get(), Some(2));
}

#[test]
fn stages_external_slot() {
    let mut slab = (external::Exclusive::<usize, Epoch>::try_new(capacity::Count::new(1)))
        .expect("the one-slot external slab must allocate");
    let key = {
        let grant = (slab.try_grant()).expect("the first external identity must be available");
        let vacant = grant.reserve();
        let key = vacant.key();
        let mut staged = vacant.stage(11);
        assert_eq!(staged.get(), &11);
        *staged.get_mut() = 13;
        let size = mem::size_of_val(&staged);
        let occupied = staged.commit();
        assert_eq!(mem::size_of_val(&occupied), size);
        assert_eq!(occupied.get(), &13);
        key
    };
    assert_eq!(slab.get(key), Some(&13));
    assert_eq!(slab.occupied(key).map(|entry| entry.remove()), Some(13));

    let grant = (slab.try_grant()).expect("the removed external slot must be available");
    assert_eq!(grant.reserve().stage(17).reject(), 17);
    assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn drops_abandoned_stage() {
    let drops = Cell::new(Some(0));
    let mut slab = (external::Exclusive::<(_, usize), Epoch>::try_new(capacity::Count::new(1)))
        .expect("the one-slot external slab must allocate");
    {
        let grant = (slab.try_grant()).expect("the first external identity must be available");
        let staged = grant.reserve().stage((Counted(&drops), 19));
        assert_eq!(staged.get().1, 19);
        let gone = staged.reject();
        assert_eq!(gone.1, 19);
    }
    assert_eq!(drops.get(), Some(1));
    assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn retains_occupancy() {
    use external::{errors, shared};
    use o3::depot::arena::slab::lookup;

    let storage = (shared::Storage::<usize, Epoch>::try_new(capacity::Count::new(1)))
        .expect("shared storage allocation");
    let vacant = (storage.vacancy()).expect("initial vacancy");
    let key = vacant.insert(11);
    let held = storage.try_entry(key);
    let mut held = held.expect("external key must retain its original logical generation");
    assert!(matches!(
        storage.try_entry(key),
        Err(errors::Lookup::Physical {
            key: seen,
            cause: lookup::Error::Busy { index: 0, borrow: () },
        }) if seen == key
    ));
    *held.get_mut() += 2;
    assert_eq!(*held.get_mut(), 13);
    *held.get_mut() += 5;
    assert_eq!(*held.get_mut(), 18);
    let value = held.take();
    assert_eq!(value, 18);
    let vacant = storage
        .vacancy()
        .expect("released capacity retains its logical cursor");
    let next = vacant.insert(23);
    assert_eq!(next.index(), key.index());
    assert!(matches!(
        storage.try_entry(key),
        Err(errors::Lookup::Generation { key: seen, actual })
            if seen == key && actual == next.generation()
    ));
    let held = storage.try_entry(next);
    let held = held.expect("external key must retain its original logical generation");
    assert_eq!(held.take(), 23);
    assert!(storage.is_empty());
}

#[test]
fn matches_reference_set() {
    const CAP: u32 = if cfg!(miri) { 128 } else { 5000 };
    let (mut s,) = (({
        let actual: Result<slab::Store<u32>, _> = slab::Store::<_>::try_new(Count::new(CAP));
        actual
    })
    .expect("the reference-model stress slab must allocate"),);
    let mut free: BTreeSet<u32> = (0..CAP).collect();
    let mut live: HashMap<u32, Handle> = HashMap::new();
    let mut rng = Lcg::<1_442_695_040_888_963_407>::new(0x1234_5678_9abc_def0);

    let edges: &[u32] = if cfg!(miri) {
        &[0, 63, 64, 127]
    } else {
        &[0, 63, 64, 127, 4095, 4096, 4999]
    };
    for &index in edges {
        let first = slab::Slots::new(&mut s)
            .vacant_at(index)
            .expect("an untouched edge slot is vacant")
            .insert(2);
        assert!(free.remove(&index));
        assert_eq!(s.remove(first), Some(2));
        free.insert(index);
        let next = slab::Slots::new(&mut s)
            .vacant_at(index)
            .expect("removing an edge restores its vacancy")
            .insert(2);
        assert!(first != next);
        assert!(free.remove(&index));
        assert!(live.insert(index, next).is_none());
    }

    let runs = if cfg!(miri) { 1_000 } else { 300_000 };
    for _ in 0..runs {
        match (rng.advance() >> 33) % 3 {
            0 => match s.insert(1) {
                Ok(key) => {
                    let idx = key.index();
                    assert!(free.remove(&idx), "insert returned occupied slot {idx}");
                    assert!(live.insert(idx, key).is_none(), "duplicate insert at {idx}");
                    assert_eq!(s.try_get(key).ok(), Some(&1));
                }
                Err(_) => assert!(free.is_empty(), "insert failed with {} free", free.len()),
            },
            1 => {
                let idx = ((rng.advance() >> 33) as u32) % CAP;
                let placed = slab::Slots::new(&mut s)
                    .vacant_at(idx)
                    .map(|entry| entry.insert(2));
                assert_eq!(
                    placed.is_some(),
                    free.contains(&idx),
                    "vacant_at({idx}) disagreed with model"
                );
                if let Some(key) = placed {
                    free.remove(&idx);
                    live.insert(idx, key);
                }
            }
            _ => {
                let victim = live.keys().next().copied();
                if let Some(idx) = victim {
                    let key = live
                        .remove(&idx)
                        .expect("an index selected from live still has its handle");
                    assert!(
                        s.remove(key).is_some(),
                        "removing live key {idx} returned None"
                    );
                    free.insert(idx);
                    assert_eq!(
                        s.try_get(key).ok(),
                        None,
                        "removed key {idx} still resolves"
                    );
                }
            }
        }
    }

    assert_eq!(free.len() as u32 + live.len() as u32, CAP);
    for (&idx, &key) in &live {
        assert!(s.try_get(key).ok().is_some(), "live key {idx} vanished");
    }
    let mut fill = 0usize;
    while s.insert(9).is_ok() {
        fill += 1;
    }
    assert_eq!(
        fill,
        free.len(),
        "filled {fill}, model has {} free",
        free.len()
    );
}

#[test]
fn restores_exact_identity() {
    let mut table =
        (slab::Store::<u64>::try_new(capacity::Count::new(3))).expect("slab must allocate");
    let (first, middle, last) = (
        (table.insert(11)).expect("three slots must insert"),
        (table.insert(23)).expect("three slots must insert"),
        (table.insert(37)).expect("three slots must insert"),
    );
    {
        let entry =
            (table.try_entry(middle)).expect("slab key must retain its original slot observation");

        let ((entry, prior), delta) = Tracker::measure(|| {
            let (entry, rest) = entry.split();
            assert!(matches!(rest.try_get(first), Ok(&11)));
            assert!(matches!(rest.try_get(last), Ok(&37)));
            let (slot, prior) = entry.take();
            (slot.insert(prior + 1), prior)
        });
        assert_eq!(delta, [0; 4]);
        assert_eq!(prior, 23);
        assert!((entry.key()) == (middle));
        assert_eq!(*entry.get(), 24);
    }
    assert!((table.len()) == (capacity::Count::new(3)));
    assert!((table.remove(first)) == (Some(11)));
    assert!((table.remove(last)) == (Some(37)));
    assert!((table.remove(middle)) == (Some(24)));
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn drop_retires_identity() {
    let drops = cell::Cell::new(Some(0));
    let mut table = (slab::Store::<Counted<&cell::Cell<Option<usize>>>, (), 2>::try_new(
        capacity::Count::new(1),
    ))
    .expect("slab must allocate");
    let first = (table.insert(Counted(&drops))).expect("slab must retain capacity for this item");
    {
        let value = {
            let entry = table
                .try_entry(first)
                .expect("slab key must retain its original slot observation");
            let taken = entry.take();
            taken.1
        };
        assert_eq!(value.0.get(), Some(0));
        assert!(table.try_get(first).ok().is_none());
    }
    assert_eq!(drops.get(), Some(1));
    let last = (table.insert(Counted(&drops))).expect("slab must retain capacity for this item");
    assert_eq!(first.index(), last.index());
    assert!((first.generation()) != (last.generation()));
    {
        let entry = table
            .try_entry(last)
            .expect("slab key must retain its original slot observation");
        let taken = entry.take();
        assert_eq!(taken.1.0.get(), Some(1));
    }
    assert_eq!(drops.get(), Some(2));
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
    assert!(table.is_full());
    assert!(matches!(
        table.try_entry(first),
        Err(slab::key::Error::Retired { .. })
    ));
}

#[test]
fn forget_preserves_storage() {
    let drops = cell::Cell::new(Some(0));
    {
        let value = {
            let mut table = (slab::Store::<Counted<&cell::Cell<Option<usize>>>>::try_new(
                capacity::Count::new(1),
            ))
            .expect("slab must allocate");
            let key =
                (table.insert(Counted(&drops))).expect("slab must retain capacity for this item");
            let entry =
                (table.try_entry(key)).expect("slab key must retain its original slot observation");

            let (slot, value) = entry.take();
            mem::forget(slot);
            assert!(matches!(
                table.try_entry(key),
                Err(slab::key::Error::Reserved { .. })
            ));
            assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
            assert!(table.is_full());
            table.clear();
            value
        };
        assert_eq!(value.0.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(1));
}

#[test]
fn splits_checked_payloads() {
    let mut storage =
        (slab::Store::<Family>::try_new(capacity::Count::new(2))).expect("slab allocation");
    let vacant = (storage.try_vacant()).expect("first vacancy");
    let first = vacant.insert(Family {
        value: Some(Box::new(7)),
        links: [const { Cell::new(0) }; 3],
    });
    let vacant = (storage.try_vacant()).expect("second vacancy");
    let second = vacant.insert(Family {
        value: Some(Box::new(11)),
        links: [const { Cell::new(0) }; 3],
    });
    {
        let mut entry =
            (storage.try_entry(first)).expect("slab key must retain its original slot observation");

        let family = entry.get_mut();
        let value = (family.value.as_mut()).expect("live payload");
        let local = &family.links[1];
        local.set(17);
        **value = 23;
        assert_eq!(local.get(), 17);
        assert_eq!(**value, 23);
    }
    {
        let mut entry = (storage.try_entry(second))
            .expect("slab key must retain its original slot observation");

        let family = entry.get_mut();
        family.links[2].set(19);
        assert_eq!(family.value.as_deref(), Some(&11));
    }
    let entry =
        (storage.try_entry(first)).expect("slab key must retain its original slot observation");

    let removed = entry.remove();
    assert_eq!(removed.value.as_deref(), Some(&23));
    assert_eq!(removed.links[1].get(), 17);
    assert!(storage.try_get(first).is_err());
    assert!(matches!(
        storage.try_entry(first),
        Err(slab::key::Error::Free { key, .. }) if key == first
    ));
}
