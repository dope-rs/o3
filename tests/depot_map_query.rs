pub mod common;

use std::cell::Cell;

use common::depot::map::{Directory, Seed};
use o3::depot::map::{self, flat, priority};

#[test]
fn bounds_raw_queries() {
    for (plan, width) in [(map::Plan::fixed::<0>(), 0), (map::Plan::fixed::<1>(), 2)] {
        let mut table = Directory::try_new(plan).expect("raw directory");
        for value in 0..width {
            let (entry, hash) = match map::raw::Probe::reserve(&mut table, 1, |_| false) {
                map::Entry::Vacant(vacant) => vacant,
                map::Entry::Occupied(()) => None,
            }
            .expect("vacant raw bucket");
            let (owner, bucket, ()) = entry.into_parts();
            // SAFETY: the consumed vacancy retains this exact owner, bucket and hash.
            unsafe { owner.table.insert(bucket, hash, value) };
        }

        let visits = Cell::new(0);
        assert!(
            map::raw::Probe::find(&mut table, 1, |value| {
                assert_eq!(*value, visits.get());
                visits.set(visits.get() + 1);
                false
            })
            .is_none()
        );
        assert_eq!(visits.get(), width);
        assert!(matches!(
            map::raw::Probe::reserve(&mut table, 1, |_| false),
            map::Entry::Vacant(None)
        ));
        if width != 0 {
            assert_eq!(
                map::raw::Probe::find(&mut table, 1, |value| *value == 1)
                    .map(|entry| entry.copied().1),
                Some(1)
            );
        }
    }
}

#[test]
fn empty_admission() {
    let mut table = flat::Table::<u32>::try_new(map::Plan::fixed::<0>()).expect("empty flat table");
    table.clear();
    let compared = Cell::new(false);
    assert!(table.get(1, |_| compared.replace(true)).is_none());
    assert!(table.entry(1, |_| compared.replace(true)).is_none());
    assert!(!compared.get());

    let plan = priority::Plan::<u64, u64, u64>::new(0).expect("empty priority plan");
    let mut map = priority::Min::try_new(plan, Seed(0)).expect("empty priority map");
    assert!(map.get(&1).is_none());
    assert_eq!(
        map.try_entry(1).err().expect("full empty map").into_parts(),
        (1, priority::errors::Full)
    );
}
