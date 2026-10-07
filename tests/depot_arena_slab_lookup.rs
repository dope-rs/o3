pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{array, convert::Infallible, mem};

use common::{
    alloc::system::Tracker,
    depot::arena::slab::external::{Epoch, Generation, Wide},
};
use o3::depot::{
    arena::slab::{
        self, entries,
        external::{self, errors, shared},
        key, lookup,
    },
    capacity,
};

#[test]
fn rejects_observed_state() {
    let result = slab::Store::<u32, (), 2>::try_new(capacity::Count::new(1));
    let mut table = result.expect("required fixture outcome");
    let first = key::Handle::from_parts(key::Parts::new(0, key::Generation::MIN));
    assert!(
        matches!(table.try_get(first), Err(key::Error::Free { key, actual }) if key == first && actual == first.generation()),
        "uninitialized slot must report its exact free generation"
    );
    let outside = key::Handle::from_parts(key::Parts::new(7, key::Generation::MIN));
    assert!(
        matches!(table.try_get(outside), Err(key::Error::Index { key, capacity }) if key == outside && capacity == capacity::Count::new(1)),
        "lookup must preserve the rejected key and capacity"
    );
    let inserted = table.insert(11);
    assert!((inserted) == (Ok(first)));
    assert_eq!(table.remove(first), Some(11));
    let inserted = table.insert(13);
    let second = inserted.expect("required fixture outcome");
    assert!(matches!(table.try_get(second), Ok(&13)));
    assert!(
        matches!(table.try_get(first), Err(key::Error::Generation { key, actual }) if key == first && actual == second.generation()),
        "stale identity must preserve both generations"
    );
    {
        let selected = table.try_entry(second);
        let mut selected = selected.expect("required fixture outcome");
        *selected.get_mut() += 4;
        assert_eq!(selected.get(), &17);
        assert_eq!(selected.remove(), 17);
    }
    assert!(
        matches!(table.try_get(second), Err(key::Error::Retired { key, actual, retired: () }) if key == second && actual == second.generation()),
        "exhausted generation must report retirement"
    );
}

#[test]
fn retains_free_links() {
    let result = slab::Store::<u32, (), 3, false, 2>::try_new(capacity::Count::new(8));
    let mut table = result.expect("required fixture outcome");
    let mut keys: [key::Handle<(), 3>; 8] = array::from_fn(|index| {
        key::Handle::from_parts(key::Parts::new(index as u32, key::Generation::MIN))
    });
    for index in [2, 0, 5, 7, 1, 6, 4, 3] {
        let vacant = slab::Slots::new(&mut table).vacant_at(index);
        let vacant = vacant.expect("assertion failed: vacant.is_some()");
        let selected = vacant.stage(index).commit();
        assert!((selected.key()) == (keys[index as usize]));
        assert_eq!(*selected.get(), index);
    }
    assert!((table.len()) == (capacity::Count::new(8)));
    for index in [1, 6, 2, 5] {
        {
            let selected = (table.try_entry(keys[index]))
                .expect("slab key must retain its original slot observation");

            assert_eq!(selected.remove(), index as u32)
        }
    }
    assert!((table.len()) == (capacity::Count::new(4)));
    for index in [5, 2, 1, 6] {
        let old = keys[index];
        {
            let vacant = slab::Slots::new(&mut table).vacant_at(index as u32);
            let vacant = vacant.expect("assertion failed: vacant.is_some()");
            let selected = vacant.stage(index as u32).commit();
            keys[index] = selected.key();
        }
        assert!(
            matches!(table.try_entry(old), Err(key::Error::Generation { key, actual }) if key == old && actual == keys[index].generation()),
            "reused slot must reject its previous generation"
        );
    }
    for (index, key) in keys.into_iter().enumerate() {
        assert_eq!(table.try_get(key).ok(), Some(&(index as u32)));
    }
    table.clear();
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
    for _ in 0..8 {
        table.insert(99).as_ref().expect("required fixture outcome");
    }
    assert!((table.len()) == (capacity::Count::new(8)));
    assert!((table.insert(100)) == (Err(100)));
}

#[test]
fn reads_disjoint_slots() {
    let mut table =
        (slab::Store::<u32, (), 2>::try_new(capacity::Count::new(2))).expect("two-slot backing");
    let first = (table.insert(11)).expect("first occupied slot");
    let old = (table.insert(13)).expect("neighbor slot");
    assert_eq!(table.remove(old), Some(13));
    let next = (table.insert(17)).expect("recycled neighbor");
    let held =
        (table.try_entry(first)).expect("slab key must retain its original slot observation");

    let (mut held, rest) = held.split();
    let current = held.get_mut();
    assert!(
        matches!(rest.try_get(first), Err(key::Error::Busy { key, actual, borrow })
        if key == first && actual == first.generation() && borrow.index == first.index())
    );
    assert!(matches!(rest.try_get(next), Ok(&17)));
    assert!(
        matches!(rest.try_get(old), Err(key::Error::Generation { key, actual })
        if key == old && actual == next.generation())
    );
    let outside = key::Handle::from_parts(key::Parts::new(7, key::Generation::MIN));
    assert!(
        matches!(rest.try_get(outside), Err(key::Error::Index { key, capacity })
        if key == outside && capacity == capacity::Count::new(2))
    );
    assert_eq!(*current, 11);
    *current = 19;
    let neighbor =
        (rest.try_get(next)).expect("slab key must retain its original slot observation");

    assert_eq!(held.remove(), 19);
    assert_eq!(*neighbor, 17);
    assert!((table.try_get(next).ok()) == (Some(&17)));
    assert!((table.remove(next)) == (Some(17)));
    let first = (table.insert(23)).expect("the other physical slot remains reusable");
    let held =
        (table.try_entry(first)).expect("slab key must retain its original slot observation");

    let (held, rest) = held.split();
    assert!(
        matches!(rest.try_get(next), Err(key::Error::Retired { key, actual, retired: () })
        if key == next && actual == next.generation())
    );
    assert_eq!(held.remove(), 23);
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn splits_disjoint_slots() {
    let mut table =
        (slab::Store::<u32>::try_new(capacity::Count::new(3))).expect("three-slot backing");
    let first = (table.insert(11)).expect("occupied predecessor");
    for index in [1, 2, 3] {
        let vacant = (table.try_vacant()).expect("reservation returned after every rejection");
        let attempt = key::Handle::from_parts(key::Parts::new(index, key::Generation::MIN));
        {
            let result = vacant.try_split(attempt);
            assert!(
                matches!(
                    result,
                    Err(key::Error::Reserved { key, actual })
                        if index == 1 && key == attempt && actual == attempt.generation()
                ) || matches!(
                    result,
                    Err(key::Error::Free { key, actual })
                        if index == 2 && key == attempt && actual == attempt.generation()
                ) || matches!(
                    result,
                    Err(key::Error::Index { key, capacity })
                        if index == 3 && key == attempt && capacity == capacity::Count::new(3)
                )
            );
        }
        assert!((table.len()) == (capacity::Count::new(1)));
    }
    let ((), counts) = Tracker::measure(|| {
        let vacant = (table.try_vacant()).expect("unissued reservation remains reusable");
        let (vacant, mut held) =
            (vacant.try_split(first)).expect("slab key must retain its original slot observation");

        let previous = held.get_mut();
        let mut inserted = vacant.stage(17).commit();
        assert_eq!(inserted.key().index(), 1);
        assert!((inserted.key().generation()) == (key::Generation::MIN));
        *inserted.get_mut() = 23;
        *previous = 19;
        assert_eq!(held.remove(), 19);
        let (vacant, value) = inserted.take();
        let restored = vacant.insert(value + 1);
        assert_eq!(restored.remove(), 24);
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn rejects_stale_split() {
    let mut table =
        (slab::Store::<u32, (), 2>::try_new(capacity::Count::new(2))).expect("two-slot backing");
    let first = (table.insert(11)).expect("first generation");
    assert_eq!(table.remove(first), Some(11));
    let last = (table.insert(13)).expect("last generation");
    let vacant = (table.try_vacant()).expect("other physical slot");
    assert!(
        matches!(vacant.try_split(first), Err(key::Error::Generation { key, actual })
        if key == first && actual == last.generation())
    );
    assert_eq!(table.remove(last), Some(13));
    let vacant = (table.try_vacant()).expect("rejected split returned the reservation");
    assert!(
        matches!(vacant.try_split(last), Err(key::Error::Retired { key, actual, retired: () })
        if key == last && actual == last.generation())
    );
    table.insert(17).as_ref().expect("required fixture outcome");
}

#[test]
fn retains_guard_niches() {
    let pair = 2 * mem::size_of::<usize>();
    assert_eq!(mem::size_of::<slab::Rest<'static, u32>>(), pair);
    assert_eq!(mem::size_of::<Option<slab::Rest<'static, u32>>>(), pair);
    assert_eq!(
        mem::size_of::<slab::Occupied<entries::Busy<'static, u32>>>(),
        pair
    );
    assert_eq!(
        mem::size_of::<Option<slab::Occupied<entries::Busy<'static, u32>>>>(),
        pair
    );
    assert_eq!(
        mem::size_of::<slab::Vacant<entries::Pending<'static, u32>>>(),
        pair
    );
    assert_eq!(
        mem::size_of::<Option<slab::Vacant<entries::Pending<'static, u32>>>>(),
        pair
    );
    assert_eq!(
        mem::size_of::<
            slab::Staged<o3::mem::Lease<entries::Busy<'static, u32>, o3::mem::lease::Open>>,
        >(),
        pair
    );
    assert_eq!(
        mem::size_of::<
            Option<slab::Staged<o3::mem::Lease<entries::Busy<'static, u32>, o3::mem::lease::Open>>>,
        >(),
        pair
    );
    assert_eq!(
        mem::size_of::<slab::Occupied<entries::Busy<'static, u32>, (), false>>(),
        pair
    );
    assert_eq!(
        mem::size_of::<Option<slab::Vacant<entries::Pending<'static, u32>, (), false>>>(),
        pair
    );
}

#[test]
fn rejects_absence() {
    let storage = (shared::Storage::<usize, Epoch>::try_new(capacity::Count::new(1)))
        .expect("shared storage allocation");
    let key = external::Key::new(0, Epoch::MIN);
    assert!(matches!(
        storage.try_entry(key),
        Err(errors::Lookup::Physical { key: seen, cause: lookup::Error::Free { index: 0 } })
            if seen == key
    ));
    let key = external::Key::new(1, Epoch::MIN);
    assert!(matches!(
        storage.try_entry(key),
        Err(errors::Lookup::Physical {
            key: seen,
            cause: lookup::Error::Index { index: 1, capacity },
        }) if seen == key && capacity == capacity::Count::new(1)
    ));
}

#[test]
fn excludes_before_identity() {
    let mut table = external::Exclusive::<(), Wide, (), 1, 2>::try_new(capacity::Count::new(5))
        .expect("odd partitioned extent");
    let mut finite =
        external::Exclusive::<(), Generation, (), 1, 2>::try_new(capacity::Count::new(2))
            .expect("two partitioned slots");
    let ((), counts) = Tracker::measure(|| {
        for (range, excluded, expected) in [
            (0..5, 0, 1),
            (0..3, 1, 0),
            (3..5, 3, 4),
            (1..4, 1, 2),
            (4..u32::MAX, 0, 4),
            (0..5, u32::MAX, 2),
            (0..u32::MAX, 2, 0),
            (3..u32::MAX, 4, 3),
        ] {
            let entry = table
                .try_except(range, excluded)
                .expect("eligible free member")
                .reserve();
            assert_eq!(entry.key().index(), expected);
        }
        for (range, excluded) in [(0..0, 0), (2..3, 2), (5..u32::MAX, 0)] {
            assert!(matches!(
                table.try_except(range, excluded),
                Err(slab::Vacancy::Full)
            ));
        }
        for epoch in 1..=3 {
            drop(
                finite
                    .try_except(0..2, 0)
                    .expect("unissued grant can be cancelled"),
            );
            let entry = finite
                .try_except(0..2, 0)
                .expect("cancellation preserves the logical cursor")
                .reserve();
            let key = entry.key();
            assert_eq!((key.index(), key.generation().get()), (1, epoch));
            entry.commit(());
            assert!(matches!(
                finite.try_except(0..2, 0),
                Err(slab::Vacancy::Full)
            ));
            finite.occupied(key).expect("committed neighbor").remove();
        }
        assert!(matches!(
            finite.try_except(0..1, 0),
            Err(slab::Vacancy::Full)
        ));
        assert!(matches!(
            finite.try_except(0..2, 0),
            Err(slab::Vacancy::IdentityExhausted)
        ));
        assert!(matches!(
            finite.try_grant(),
            Err(slab::Vacancy::IdentityExhausted)
        ));
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn preserves_layout() {
    type Borrowed = external::Borrowed<
        entries::Busy<
            'static,
            external::Stamped<usize, Epoch>,
            { u32::MAX },
            entries::Interior<true>,
            1,
        >,
        (),
        true,
    >;
    assert_eq!(mem::size_of::<Borrowed>(), 2 * mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<Option<Borrowed>>(),
        mem::size_of::<Borrowed>()
    );
    assert_eq!(
        mem::size_of::<lookup::Error<()>>(),
        mem::size_of::<lookup::Error>()
    );
    assert_eq!(
        mem::size_of::<lookup::Error<(), Infallible>>(),
        mem::size_of::<lookup::Error>()
    );
    assert_eq!(
        mem::size_of::<lookup::Error<Infallible, Infallible>>(),
        mem::size_of::<lookup::Error>()
    );
}
