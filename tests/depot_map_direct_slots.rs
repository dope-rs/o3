pub mod common;

use std::{cell::Cell, mem, num::NonZero};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{index::Id, model::Lcg},
};
use o3::depot::{
    capacity,
    map::direct::{Occupied, Table, Vacant},
    raw::Key as _,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn keeps_allocation_bounds() {
    const CAPACITY: usize = 257;
    let bytes = CAPACITY * 2 * mem::size_of::<u64>();
    let capacity = capacity::Count::new(CAPACITY as u32);
    let (slots, counts) = Tracker::measure(|| Table::<Id, u64>::try_new(capacity));
    assert_eq!(counts, [1, bytes, 0, 0]);
    let slots = (slots).expect("the 257-entry direct table must allocate");
    assert_eq!(mem::size_of_val(&slots), 2 * mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<Vacant<'static, Id, u64>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Occupied<'static, Id, u64>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<Occupied<'static, Id, u64>>>(),
        mem::size_of::<Occupied<'static, Id, u64>>()
    );
    assert_eq!(
        mem::size_of::<Option<Vacant<'static, Id, u64>>>(),
        mem::size_of::<Vacant<'static, Id, u64>>()
    );

    let (slots, counts) = Tracker::measure(|| {
        let mut slots = slots;
        for index in 0..CAPACITY {
            slots
                .vacant(Id::new(
                    u32::try_from(index).expect("fixture index"),
                    NonZero::<u32>::MIN,
                ))
                .expect("the selected slot must be vacant")
                .insert(index as u64);
        }
        for index in 0..CAPACITY {
            let key = Id::new(
                u32::try_from(index).expect("fixture index"),
                NonZero::<u32>::MIN,
            );
            assert_eq!(slots.get(key), Some(&(index as u64)));
            *slots
                .entry(key)
                .expect("the preceding insert occupies each exact key")
                .into_mut() += 1;
        }
        for index in (0..CAPACITY).step_by(2) {
            assert_eq!(
                slots
                    .entry(Id::new(
                        u32::try_from(index).expect("fixture index"),
                        NonZero::<u32>::MIN,
                    ))
                    .map(|entry| entry.remove()),
                Some(index as u64 + 1)
            );
        }
        slots
    });
    assert_eq!(counts, [0; 4]);

    let ((), counts) = Tracker::measure(move || {
        assert_eq!(slots.len(), CAPACITY / 2);
        assert_eq!(slots.get(Id::new(1, NonZero::<u32>::MIN)), Some(&2));
        assert_eq!(
            slots.get(Id::new(
                u32::try_from(CAPACITY - 2).expect("fixture index"),
                NonZero::<u32>::MIN,
            )),
            Some(&256)
        );
    });
    assert_eq!(counts, [0, 0, 1, bytes]);

    let (empty, counts) = Tracker::measure(|| Table::<Id, u64>::try_new(capacity::Count::EMPTY));
    assert_eq!(counts, [0; 4]);
    let empty = (empty).expect("the empty direct table must construct without allocation");
    let ((), counts) = Tracker::measure(move || {
        assert!(empty.is_empty());
        assert_eq!(empty.capacity(), 0);
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn distinguishes_exact_keys() {
    let mut slots = (Table::try_new(capacity::Count::new(2)))
        .expect("the two-entry direct table must allocate");
    let first = Id::new(0, NonZero::<u32>::MIN);
    let stale = Id::new(0, NonZero::new(2).expect("fixture epoch"));
    let outside = Id::new(2, NonZero::<u32>::MIN);
    assert!(slots.vacant(outside).is_none());
    assert!(slots.entry_at(1).is_none());
    assert!(slots.entry_at(2).is_none());

    slots
        .vacant(first)
        .expect("the first exact key addresses a vacant slot")
        .insert(7);
    assert!(slots.vacant(first).is_none());
    assert_eq!(slots.get(stale), None);
    assert_eq!(slots.get(outside), None);
    assert_eq!(slots.entry(stale).map(|entry| entry.remove()), None);
    assert!(slots.key_at(0) == Some(first));
    assert_eq!(slots.get(first), Some(&7));

    assert_eq!(slots.entry(first).map(|entry| entry.remove()), Some(7));
    assert!(slots.key_at(0).is_none());
    slots
        .vacant(stale)
        .expect("the selected slot must be vacant")
        .insert(11);
    assert_eq!(slots.get(first), None);
    assert_eq!(slots.get(stale), Some(&11));

    slots.clear();
    assert!(slots.is_empty());
    assert!(slots.key_at(0).is_none());
    slots
        .vacant(first)
        .expect("the selected slot must be vacant")
        .insert(12);
    assert_eq!(slots.get(first), Some(&12));
}

#[test]
fn retains_removed_slot() {
    let drops = Cell::new(Some(0));
    let mut slots =
        (Table::try_new(capacity::Count::new(2))).expect("the direct table must allocate");
    let key = Id::new(0, NonZero::new(9).expect("fixture epoch"));
    slots
        .vacant(key)
        .expect("the selected slot must be vacant")
        .insert(Counted(&drops));
    let ((), counts) = Tracker::measure(|| {
        let occupied = (slots.entry(key)).expect("the inserted key must remain occupied");
        let (vacant, value) = occupied.take();
        assert!(vacant.key() == key);
        assert_eq!(drops.get(), Some(0));
        vacant.insert(value);
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(slots.len(), 1);
    assert!(slots.get(key).is_some());
    slots.clear();
    assert_eq!(drops.get(), Some(1));
    assert!(slots.is_empty());
}

#[test]
fn drops_each_value_once() {
    let drops = Cell::new(Some(0));
    {
        let mut slots = (Table::try_new(capacity::Count::new(2)))
            .expect("the two-entry counted table must allocate");
        let first = Id::new(0, NonZero::<u32>::MIN);
        let second = Id::new(1, NonZero::<u32>::MIN);
        slots
            .vacant(first)
            .expect("the selected slot must be vacant")
            .insert(Counted(&drops));
        slots
            .vacant(second)
            .expect("the selected slot must be vacant")
            .insert(Counted(&drops));
        {
            let removed = slots
                .entry(first)
                .expect("the first counted key was inserted")
                .remove();
            assert_eq!(drops.get(), Some(0));
            assert!(std::ptr::eq(removed.0, &drops));
            assert!(slots.get(first).is_none());
            assert!(slots.get(second).is_some());
        }
        assert_eq!(drops.get(), Some(1));
        slots.clear();
        slots.clear();
        assert_eq!(drops.get(), Some(2));
        assert!(slots.is_empty());
        slots
            .vacant(first)
            .expect("the selected slot must be vacant")
            .insert(Counted(&drops));
        assert_eq!(slots.len(), 1);
    }
    assert_eq!(drops.get(), Some(3));
}

#[test]
fn iterates_keys_and_values() {
    let mut slots = (Table::try_new(capacity::Count::new(5)))
        .expect("the five-entry direct table must allocate");
    let keys = [
        Id::new(1, NonZero::new(3).expect("fixture epoch")),
        Id::new(4, NonZero::new(7).expect("fixture epoch")),
    ];
    for (key, value) in keys.into_iter().zip([10, 20]) {
        slots
            .vacant(key)
            .expect("the selected slot must be vacant")
            .insert(value);
    }
    for key in keys {
        let entry = (slots.entry(key)).expect("the inserted exact key remains occupied");
        *entry.into_mut() += key.index() as u64;
    }

    assert!(
        slots
            .iter()
            .map(|(key, value)| (key, *value))
            .collect::<Vec<_>>()
            == [(keys[0], 11), (keys[1], 24)]
    );
}

#[test]
fn matches_churn_model() {
    const CAPACITY: usize = 31;
    const OPERATIONS: usize = 50_000;
    let mut slots = (Table::try_new(capacity::Count::new(CAPACITY as u32)))
        .expect("the churn-model direct table must allocate");
    let mut model = vec![None; CAPACITY];
    let mut rng = Lcg::<1_442_695_040_888_963_407>::new(0x243f_6a88_85a3_08d3_u64);

    for turn in 0..OPERATIONS {
        let random = rng.advance();
        let index = (random as usize >> 9) % (CAPACITY + 4);
        let epoch = ((random >> 32) as u32 % 19) + 1;
        let key = Id::new(
            u32::try_from(index).expect("fixture index"),
            NonZero::new(epoch).expect("fixture epoch"),
        );
        let value = random.rotate_left(23);

        match random % 6 {
            0 => {
                if index < CAPACITY && model[index].is_none() {
                    slots
                        .vacant(key)
                        .expect("the selected slot must be vacant")
                        .insert(value);
                    model[index] = Some((key, value));
                    assert_eq!(slots.get(key), Some(&value));
                }
            }
            1 => {
                let expected = model.get(index).and_then(|entry| match entry {
                    Some((current, value)) if *current == key => Some(value),
                    _ => None,
                });
                assert_eq!(slots.get(key), expected);
            }
            2 => {
                let expected = model.get_mut(index).and_then(|entry| match entry {
                    Some((current, value)) if *current == key => {
                        *value = value.wrapping_add(turn as u64);
                        Some(*value)
                    }
                    _ => None,
                });
                let actual = slots.entry(key).map(|entry| {
                    let stored = entry.into_mut();
                    *stored = stored.wrapping_add(turn as u64);
                    *stored
                });
                assert_eq!(actual, expected);
            }
            3 => {
                let expected = model.get_mut(index).and_then(|entry| {
                    if entry.as_ref().is_some_and(|entry| entry.0 == key) {
                        entry.take().map(|entry| entry.1)
                    } else {
                        None
                    }
                });
                assert_eq!(slots.entry(key).map(|entry| entry.remove()), expected);
            }
            4 => {
                let expected = model
                    .get(index)
                    .and_then(|entry| entry.as_ref().map(|entry| entry.0));
                assert!(slots.key_at(index) == expected);
            }
            _ if random & 0xfff == 0 => {
                slots.clear();
                model.fill(None);
            }
            _ => assert_eq!(
                slots.get(key).is_some(),
                model.get(index).is_some_and(|entry| {
                    entry.as_ref().is_some_and(|(stored, _)| *stored == key)
                })
            ),
        }

        assert_eq!(
            slots.len(),
            model.iter().filter(|entry| entry.is_some()).count()
        );
    }

    assert!(
        slots
            .iter()
            .map(|(key, value)| (key, *value))
            .eq(model.iter().filter_map(|entry| *entry))
    );
}
