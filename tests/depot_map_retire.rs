pub mod common;

use std::{cell::Cell, num::NonZero};

use common::{Counted, Owner};
use o3::depot::map::{Plan, flat::Table};

#[test]
fn retires_collision_once() {
    let drops = [Cell::new(Some(0)), Cell::new(Some(0))];
    let mut table = Table::try_new(Plan::fixed::<2>()).expect("table");
    for (value, drops) in drops.iter().enumerate() {
        assert!(
            table
                .try_insert(
                    7,
                    Owner {
                        drops: Counted(drops),
                        value,
                    },
                    |_| false,
                )
                .is_ok()
        );
    }
    table.retire(7, |value| value.value == 0);
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1), Some(0)]);
    assert_eq!(table.len(), 1);
    assert!(table.get(7, |value| value.value == 0).is_none());
    drop(table.remove(7, |value| value.value == 1).expect("survivor"));
    drop(table);
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn resident_rounds_bounds() {
    use common::depot::map::ResidentHash;
    use o3::{
        base::num::bounded,
        depot::map::{Entry, resident},
    };

    let hash = Cell::new(0);
    for records in [0, 1, 3, 4, 5] {
        let plan = resident::Plan::counts([
            bounded::U32::<0, 5>::new(records).expect("bounded record count")
        ]);
        let mut table = resident::Table::<_, _, 1>::try_new(plan, ResidentHash(&hash))
            .expect("small resident table");
        for slot in 0..records {
            let entry = table.entry(resident::Id::mask(u64::from(slot)));
            assert!(matches!(&entry, Entry::Vacant(Some(_))));
            if let Entry::Vacant(Some(entry)) = entry {
                entry.insert(slot);
            }
        }
        assert!(matches!(
            table.entry(resident::Id::mask(u64::from(records))),
            Entry::Vacant(None)
        ));
    }
}

#[test]
fn resident_keeps_records() {
    use common::{Counted, depot::map::ResidentHash};
    use o3::{
        base::num::bounded,
        depot::map::{Entry, resident},
    };

    let drops = [const { Cell::new(Some(0)) }; 3];
    let hash = Cell::new(0);
    let plan = resident::Plan::counts([bounded::U32::<0, 2>::exact::<2>()]);
    let mut table =
        resident::Table::<_, _, 1>::try_new(plan, ResidentHash(&hash)).expect("two records");
    let first = resident::Id::new(1).expect("first id");
    let second = resident::Id::new(2).expect("second id");
    let stale = resident::Key::<1>::from_raw(NonZero::<u64>::MIN).expect("initial coordinate");
    let (removed, retained) = table.scope(|mut scope| {
        let entry = match scope.entry(first) {
            Entry::Vacant(entry) => entry,
            Entry::Occupied(_) => None,
        }
        .expect("unpublished record");
        drop(
            entry
                .insert((0, Counted(&drops[0])))
                .release()
                .expect("unpublished removal"),
        );
        let entry = match scope.entry(first) {
            Entry::Vacant(entry) => entry,
            Entry::Occupied(_) => None,
        }
        .expect("reused record");
        let (lease, fresh) = entry.insert((1, Counted(&drops[1]))).publish();
        assert_eq!(lease.key().raw().get(), (1 << 32) | 1);
        assert!(scope.exact(stale, first).is_err());
        let entry = match scope.entry(second) {
            Entry::Vacant(entry) => entry,
            Entry::Occupied(_) => None,
        }
        .expect("remaining record");
        let (retained, _) = entry.insert((2, Counted(&drops[2]))).publish();
        let (again, repeated) = scope
            .exact(lease.key(), first)
            .ok()
            .expect("published entry")
            .publish();
        assert!(repeated.is_none());
        scope.get_mut(&lease).0 = 7;
        assert_eq!(scope.get_mut(&again).0, 7);
        assert!(
            scope
                .exact(lease.key(), first)
                .ok()
                .expect("existing entry")
                .release()
                .is_none()
        );
        hash.set(3);
        let mut retired = scope.retire();
        drop(retired.occupied(fresh.expect("new authority")).remove());
        (lease.key(), retained.key())
    });
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1), Some(1), Some(0)]);
    assert!(table.get(stale).is_none());
    assert!(table.get(removed).is_none());
    assert_eq!(table.available(), 0);
    assert!(matches!(table.entry(first), Entry::Vacant(None)));
    let owner = table
        .exact(retained, first)
        .err()
        .expect("identity rejected");
    assert_eq!(
        owner
            .exact(retained, second)
            .ok()
            .expect("owner retained")
            .get()
            .0,
        2
    );
    drop(table);
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 3]);
}
