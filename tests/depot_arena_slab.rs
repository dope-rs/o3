pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, convert, hint, mem, num};

use common::{Counted, alloc::system::Tracker, depot::arena::slab::external::Wide};
use o3::depot::{
    self,
    arena::slab::{
        self, entries,
        external::{self, shared},
        key::{Generation, Parts},
    },
    capacity::{self, Count},
    index::Initialized,
};

#[test]
fn staging_is_unpublished() {
    let slab = (slab::Cell::<u32, (), { u32::MAX }, false>::try_new(Count::new(1)))
        .expect("the cell slab must admit one slot");
    assert_eq!(slab.capacity(), 1);
    let vacancy = (slab.vacancy()).expect("the initial slot is vacant");
    let mut staged = vacancy.stage(3);
    let key = staged.key();
    assert!(slab.entry(key.parts()).is_none());
    *staged.get_mut() = 5;
    staged.publish();
    assert_eq!(slab.update(key, |value| *value), Some(5));
    let entry = (slab.entry(key.parts())).expect("publishing exposes the exact generation");
    entry.discard();
    let vacancy = (slab.vacancy()).expect("discard restores the physical slot");
    let key = {
        let mut staged = vacancy.stage(11);
        assert_eq!(*staged.get_mut(), 11);
        staged.key()
    };
    assert!(slab.entry(key.parts()).is_none());
    assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY);
    let vacancy = (slab.vacancy()).expect("cancelled staging restores capacity");
    assert!((vacancy.insert(13).generation()) != (key.generation()));
}

#[test]
fn reuse_and_capacity() {
    let mut slab = (slab::Store::<&str>::try_new(Count::new(3)))
        .expect("the exclusive slab must admit three slots");
    assert!(!slab.is_full());
    let first = slab.insert("a").expect("first slot available");
    let old = slab.insert("b").expect("second slot available");
    let last = slab.insert("c").expect("third slot available");
    assert!(slab.is_full());
    assert!(slab.insert("overflow").is_err());

    assert_eq!(slab.remove(old), Some("b"));
    assert!(!slab.is_full());
    let next = slab.insert("B").expect("removed slot becomes available");
    assert!(slab.is_full());
    assert_eq!(next.index(), old.index());
    assert!((next.generation()) != (old.generation()));
    assert_eq!(slab.try_get(old).ok(), None);
    assert_eq!(slab.remove(old), None);
    assert_eq!(slab.try_get(next).ok(), Some(&"B"));

    {
        let mut entry =
            (slab.try_entry(last)).expect("slab key must retain its original slot observation");

        *entry.get_mut() = "C";
    }
    assert_eq!(slab.try_get(first).ok(), Some(&"a"));
    assert_eq!(slab.try_get(last).ok(), Some(&"C"));
}

#[test]
fn entry_paths_preserve() {
    let mut slab = (slab::Store::<u32>::try_new(Count::new(200)))
        .expect("the exclusive slab must admit two hundred slots");
    let entry = slab.try_vacant().expect("initial vacancy");
    let first = entry.insert(1);
    let second = slab.insert(2).expect("second slot available");
    {
        let mut entry =
            (slab.try_entry(second)).expect("slab key must retain its original slot observation");

        *entry.get_mut() = 3;
    }
    assert_eq!(slab.try_get(first).ok(), Some(&1));
    assert_eq!(slab.try_get(second).ok(), Some(&3));

    let free = slab::Slots::new(&mut slab)
        .vacant_at(130)
        .expect("high vacancy");
    let high = free.insert_with(|key| key.index());
    let free = slab::Slots::new(&mut slab)
        .vacant_at(70)
        .expect("middle vacancy");
    let middle = free.insert_with(|key| key.index());
    assert!(slab::Slots::new(&mut slab).vacant_at(70).is_none());
    let next = slab.insert(4).expect("next free slot available");
    assert_eq!(next.index(), 2);

    {
        let entry =
            (slab::Slots::new(&mut slab).entry_at(130)).expect("the high slot remains live");
        assert!((*entry.get(), entry.key()) == (130, high));
    }
    let found = {
        let mut entry = slab::Slots::new(&mut slab)
            .entry_at(70)
            .expect("middle slot remains live");
        *entry.get_mut() += 1;
        entry.key()
    };
    assert!((found) == (middle));
    assert_eq!(slab.try_get(middle).ok(), Some(&71));

    assert_eq!(slab.remove(high), Some(130));
    let next = slab::Slots::new(&mut slab)
        .vacant_at(130)
        .expect("removed high slot becomes vacant")
        .insert(7);
    assert!((high) != (next));
    assert_eq!(slab.try_get(next).ok(), Some(&7));
}

#[test]
fn range_stays_partitioned() {
    let mut slab = (slab::Store::<u32>::try_new(Count::new(6)))
        .expect("the exclusive slab must admit six slots");
    let first = slab::Slots::new(&mut slab)
        .vacant_at(2)
        .expect("slot two vacant")
        .insert(2);
    let second = slab::Slots::new(&mut slab)
        .vacant_at(3)
        .expect("slot three vacant")
        .insert(3);

    let pick = slab::Slots::new(&mut slab)
        .vacant_in(2..5)
        .expect("free slot in range");
    let pick = pick.insert_with(|key| {
        assert_eq!(key.index(), 4);
        4
    });
    assert_eq!(slab.try_get(pick).ok(), Some(&4));

    assert!(slab::Slots::new(&mut slab).vacant_in(2..4).is_none());
    assert!(slab::Slots::new(&mut slab).vacant_in(5..5).is_none());
    let last = slab::Slots::new(&mut slab)
        .vacant_in(5..6)
        .expect("last range has one vacancy")
        .insert(5);
    assert_eq!(last.index(), 5);
    assert_eq!(slab.remove(last), Some(5));

    assert_eq!(slab.remove(first), Some(2));
    assert_eq!(slab.remove(second), Some(3));
}

#[test]
fn range_stops_at_capacity() {
    let mut slab = slab::Store::<u32>::try_new(Count::new(3)).expect("three slots");
    let key = slab::Slots::new(&mut slab)
        .vacant_in(2..u32::MAX)
        .expect("last slot")
        .insert(7);
    assert_eq!(key.index(), 2);
    assert!(slab::Slots::new(&mut slab).vacant_in(2..u32::MAX).is_none());
    assert!(slab::Slots::new(&mut slab).vacant_in(3..u32::MAX).is_none());
}

#[test]
fn entry_proves_generation() {
    let mut slab = (slab::Store::<u32>::try_new(Count::new(2)))
        .expect("the exclusive slab must admit two slots");
    let key = slab.insert(7).expect("first slot");

    {
        let mut entry =
            (slab.try_entry(key)).expect("slab key must retain its original slot observation");

        assert!((entry.key()) == (key));
        assert_eq!(entry.get(), &7);
        *entry.get_mut() = 11;
    }
    assert_eq!(slab.try_get(key).ok(), Some(&11));

    let entry = slab::Slots::new(&mut slab)
        .entry_at(key.index())
        .expect("occupied index");
    assert_eq!(entry.remove(), 11);
    assert!(slab.try_entry(key).is_err());

    let next = slab.insert(13).expect("recycled slot");
    assert_eq!(next.index(), key.index());
    assert!((next.generation()) != (key.generation()));
    assert!(slab.try_entry(key).is_err());
    assert!(matches!(slab.try_entry(next), Ok(entry) if entry.get() == &13));

    let key2 = {
        let entry = slab::Slots::new(&mut slab)
            .vacant_at(1)
            .expect("the second slot is vacant")
            .stage(17)
            .commit();
        assert_eq!(entry.get(), &17);
        entry.key()
    };
    assert_eq!(slab.try_get(key2).ok(), Some(&17));
}

#[test]
fn vacancy_keeps_generation() {
    let mut slab = (slab::Store::<u32>::try_new(Count::new(4)))
        .expect("the exclusive slab must admit four slots");
    let entry = slab::Slots::new(&mut slab)
        .vacant_at(1)
        .expect("slot one vacant")
        .insert(7);
    assert!(
        slab::Slots::new(&mut slab)
            .vacant_at(entry.index())
            .is_none()
    );
    assert!(slab::Slots::new(&mut slab).vacant_at(4).is_none());

    assert!(slab::Slots::new(&mut slab).vacant_at(3).is_some());

    let next = slab::Slots::new(&mut slab)
        .vacant_at(3)
        .expect("cancelled slot stays vacant")
        .insert(9);
    assert_eq!(next.index(), 3);
    assert_eq!(next.generation().get(), 1);
    assert_eq!(slab.try_get(next).ok(), Some(&9));
    assert_eq!(slab.insert(11).expect("slot zero vacant").index(), 0);
}

#[test]
fn sparse_live_entries() {
    let mut slab = slab::Store::<u32>::try_new(Count::new(128))
        .expect("the exclusive slab must admit one hundred twenty-eight slots");
    assert!((slab::Slots::new(&slab).key_at(127)).is_none());
    let high = slab::Slots::new(&mut slab)
        .vacant_at(127)
        .expect("high slot vacant")
        .insert(7);
    let low = slab::Slots::new(&mut slab)
        .vacant_at(1)
        .expect("low slot vacant")
        .insert(3);
    assert_eq!(slab.try_get(high).ok(), Some(&7));
    assert_eq!(slab.try_get(low).ok(), Some(&3));
    assert_eq!(slab.remove(high), Some(7));
    assert_eq!(slab.try_get(high).ok(), None);
    assert_eq!(slab.try_get(low).ok(), Some(&3));

    let mut entry = slab::Slots::new(&mut slab)
        .entry_at(low.index())
        .expect("low slot remains occupied");
    *entry.get_mut() += 1;
    let epoch = entry.key().generation();
    let value = entry.remove();
    assert_eq!(value, 4);
    assert!((epoch) == (low.generation()));

    let live = slab.insert(9).expect("removed low slot becomes vacant");
    slab.clear();
    assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY);
    assert!((slab::Slots::new(&slab).key_at(live.index() as usize)).is_none());
}

#[test]
fn parts_track_generation() {
    const MAX: u32 = 7;

    assert!(Generation::<MAX>::MIN.checked_add(MAX).is_none());
    let epoch = (Generation::<MAX>::MIN.checked_add(MAX - 1)).expect("maximum generation is valid");
    let edge = Parts::new(u32::MAX, epoch);
    assert_eq!(edge.index(), u32::MAX);
    assert!((edge.generation()) == (epoch));
    assert!((Parts::from_raw(edge.into_raw())) == (Some(edge)));
    const FIRST: Parts = Parts::new(0, Generation::<MAX>::MIN).widen();
    assert_eq!(FIRST.index(), 0);
    assert_eq!(FIRST.generation().get(), 1);
    for parts in hint::black_box([Parts::new(0, Generation::<MAX>::MIN), edge]) {
        assert_eq!(parts.widen::<MAX>(), parts);
        let next = parts.widen::<{ MAX + 1 }>();
        let full: Parts = hint::black_box(next.widen());
        assert_eq!(full.into_raw(), parts.into_raw());
        assert_eq!(full.index(), parts.index());
        assert_eq!(full.generation().get(), parts.generation().get());
    }
}

#[unsafe(export_name = "external_proof_read")]
fn read(
    entry: &external::Occupied<
        entries::Busy<'_, external::Stamped<u64, Wide>, { u32::MAX }, entries::Exclusive<true>, 1>,
        (),
        true,
    >,
) -> u64 {
    *entry.get()
}

#[unsafe(export_name = "external_proof_write")]
fn write(
    entry: &mut external::Occupied<
        entries::Busy<'_, external::Stamped<u64, Wide>, { u32::MAX }, entries::Exclusive<true>, 1>,
        (),
        true,
    >,
    value: u64,
) {
    *entry.get_mut() = value;
}

#[test]
fn lookup_retains_storage() {
    assert_eq!(
        mem::size_of::<
            external::Occupied<
                entries::Busy<
                    '_,
                    external::Stamped<u64, Wide>,
                    { u32::MAX },
                    entries::Exclusive<true>,
                    1,
                >,
                (),
                true,
            >,
        >(),
        16
    );
    assert_eq!(
        mem::size_of::<
            Option<
                external::Occupied<
                    entries::Busy<
                        '_,
                        external::Stamped<u64, Wide>,
                        { u32::MAX },
                        entries::Exclusive<true>,
                        1,
                    >,
                    (),
                    true,
                >,
            >,
        >(),
        16
    );
    let (built, cold) =
        Tracker::measure(|| external::Exclusive::<u64, Wide>::try_new(capacity::Count::new(4)));
    let mut slab = (built).expect("four external slots must allocate");
    assert_eq!(cold, [1, 144, 0, 0]);
    let grant = (slab.try_grant_at(3)).expect("the last slot must be vacant");
    let key = {
        let vacant = grant.reserve();
        let key = vacant.key();
        vacant.commit(0);
        key
    };
    let ((), hot) = Tracker::measure(|| {
        let entry = slab.try_occupied(key);
        let mut entry = entry.expect("external key must retain its original logical generation");
        for value in 0..1024 {
            write(hint::black_box(&mut entry), value);
            assert_eq!(read(hint::black_box(&entry)), value);
        }
        assert_eq!(entry.remove(), 1023);
    });
    assert_eq!(hot, [0; 4]);
    let ((), freed) =
        Tracker::measure(move || assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY));
    assert_eq!(freed, [0, 0, 1, 144]);
}

#[test]
fn retains_rollback_owner() {
    let drops = Cell::new(Some(0));
    {
        let mut slab =
            (external::Exclusive::<(u32, _), Wide, (), 1>::try_new(capacity::Count::new(1)))
                .expect("one external slot must allocate");
        let grant = (slab.try_grant_at(0)).expect("initial slot must be vacant");
        let vacant = grant.reserve();
        let key = vacant.key();
        vacant.commit((0, Counted(&drops)));
        let ((), hot) = Tracker::measure(|| {
            {
                let entry = slab.try_occupied(key);
                let mut entry =
                    entry.expect("external key must retain its original logical generation");
                for expected in 1..=64 {
                    let result = entry.try_remove(|mut value| {
                        value.0 += 1;
                        Err::<convert::Infallible, _>((
                            value,
                            num::NonZero::<u32>::MIN.saturating_add(expected),
                        ))
                    });
                    entry = match result {
                        Ok(never) => match never {},
                        Err((entry, cause)) => {
                            assert_eq!(cause.get(), expected + 1);
                            entry
                        }
                    };
                    assert_eq!(entry.get().0, expected);
                    assert_eq!(drops.get(), Some(0));
                }
            }
            let entry = slab.try_occupied(key);
            let entry = entry.expect("external key must retain its original logical generation");
            match entry.try_remove(Ok::<_, (_, convert::Infallible)>) {
                Ok(value) => {
                    assert_eq!(value.0, 64);
                }
                Err((_, never)) => match never {},
            }
            assert_eq!(drops.get(), Some(1));
        });
        assert_eq!(hot, [0; 4]);
        assert_eq!(slab.len(), o3::depot::capacity::Count::EMPTY);
        let grant = (slab.try_grant_at(0)).expect("retirement must release the physical slot");
        let vacant = grant.reserve();
        assert_eq!(vacant.key().index(), key.index());
        assert_eq!(vacant.key().generation().get(), key.generation().get() + 1);
        vacant.commit((65, Counted(&drops)));
    }
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn allocated_tracks_state() {
    use o3::depot::arena::slab::lookup;

    let stored =
        shared::Storage::<u32, Wide>::try_new(capacity::Count::new(1)).expect("one physical slot");
    // SAFETY: stored owns the one-element allocation and zero selects its backing slot.
    let place: depot::raw::Proof<_, Initialized<entries::Allocated>> =
        unsafe { depot::raw::Proof::<(_, _), Initialized<entries::Allocated>>::new((&stored, 0)) }
            .into();
    let vacant = stored.vacancy().expect("initial reservation");
    assert!(matches!(
        place.try_entry(),
        Err(lookup::Error::Reserved { index: 0 })
    ));
    let key = vacant.insert(31);

    // SAFETY: the same retained allocation still owns physical position zero.
    let place: depot::raw::Proof<_, Initialized<entries::Allocated>> =
        unsafe { depot::raw::Proof::<(_, _), Initialized<entries::Allocated>>::new((&stored, 0)) }
            .into();
    let current = stored.try_entry(key).expect("live identity");
    assert!(matches!(
        place.try_entry(),
        Err(lookup::Error::Busy {
            index: 0,
            borrow: ()
        })
    ));
    assert_eq!(current.take(), 31);

    // SAFETY: removing the payload preserves stored's immutable backing allocation.
    let place: depot::raw::Proof<_, Initialized<entries::Allocated>> =
        unsafe { depot::raw::Proof::<(_, _), Initialized<entries::Allocated>>::new((&stored, 0)) }
            .into();
    let fresh = place
        .vacancy()
        .expect("recycled physical position")
        .insert(47);
    // SAFETY: stored still owns the exact admitted position after logical replacement.
    let place: depot::raw::Proof<_, Initialized<entries::Allocated>> =
        unsafe { depot::raw::Proof::<(_, _), Initialized<entries::Allocated>>::new((&stored, 0)) }
            .into();
    assert!(
        matches!(place.try_key(key.generation()), Err(external::errors::Lookup::Generation { key: old, actual }) if old == key && actual == fresh.generation())
    );
    assert_eq!(stored.try_entry(fresh).expect("fresh identity").take(), 47);
}
