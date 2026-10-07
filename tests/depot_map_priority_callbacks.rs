pub mod common;

#[test]
fn retains_physical_entry() {
    use common::depot::map::Collision;
    use o3::depot::{map::priority, raw::Proof};

    let plan = priority::Plan::new(3).expect("three residents fit");
    let mut map = priority::Min::try_new(plan, Collision).expect("priority map allocates");
    let mut selected = None;
    for key in 1..=3_u64 {
        let entry = map.try_entry(key).expect("resident capacity");
        assert!(matches!(&entry, priority::Entry::Vacant(_)));
        if let priority::Entry::Vacant(entry) = entry {
            if key == 3 {
                selected = Some(entry.index());
            }
            entry.insert(key, key * 10);
        }
    }
    assert_eq!(map.remove(&1), Some(10));
    let index = selected.expect("selected physical identity");
    // SAFETY: this exact map still owns the selected resident; only another node retired.
    let mut entry: priority::Occupied<'_, _, _, _> =
        unsafe { Proof::new((&mut map, index)) }.into();
    assert_eq!(entry.index(), index);
    assert_eq!(*entry.key(), 3);
    entry.set(0);
    *entry.get_mut() = 31;
    let entry = map.peek_mut().expect("new minimum");
    assert_eq!(entry.index(), index);
    assert_eq!(entry.remove(), 31);
    assert_eq!(map.pop(), Some((2, 2, 20)));
    assert!(map.is_empty());
}

#[test]
fn orders_before_release() {
    use std::cell::Cell;

    use common::depot::{heap::observed::Priority, map::Seed};
    use o3::depot::map::priority;

    let calls = Cell::new(0);
    let released = Cell::new(None);
    let drops = Cell::new(0);
    let compare = || calls.set(calls.get() + 1);
    let release = || {
        drops.set(drops.get() + 1);
        released.set(Some(calls.get()));
    };
    let plan = priority::Plan::new(3).expect("three residents fit");
    let mut map = priority::Min::try_new(plan, Seed(7)).expect("priority map allocates");
    for key in 0..2 {
        let priority::Entry::Vacant(entry) = map.try_entry(key).expect("capacity") else {
            panic!("occupied");
        };
        entry.insert(Priority::new(10 + key * 10, &compare, &release), key);
    }
    calls.set(0);
    let priority::Entry::Vacant(entry) = map.try_entry(2).expect("capacity") else {
        panic!("occupied");
    };
    entry.insert(Priority::new(5, &compare, &release), 2);
    assert!(calls.get() > 0);
    assert_eq!(drops.get(), 0);
    assert_eq!(
        map.peek()
            .map(|(key, priority, value)| (*key, priority.value, *value)),
        Some((2, 5, 2))
    );

    calls.set(0);
    map.occupied(&0)
        .expect("original resident")
        .set(Priority::new(1, &compare, &release));
    assert!(calls.get() > 0);
    assert_eq!(released.get(), Some(calls.get()));
    assert_eq!(drops.get(), 1);
    assert_eq!(
        map.peek()
            .map(|(key, priority, value)| (*key, priority.value, *value)),
        Some((0, 1, 0))
    );
    assert_eq!(map.len(), 3);
    drop(map);
    assert_eq!(drops.get(), 4);
}
