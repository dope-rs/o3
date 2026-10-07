pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::collections::VecDeque;

use common::{
    alloc::system::Tracker,
    depot::{
        index::capacity::Count,
        model::Lcg,
        queue::indexed::slab::{Domain, READY, Tag},
    },
};
use o3::depot::{
    capacity,
    queue::{indexed::slab, tagged},
};

#[test]
fn allocation_is_compact() {
    let (cell, counts) = Tracker::measure(|| {
        slab::Slab::<usize, Tag, { u32::MAX }, tagged::Hook, 2, _>::try_from(
            Count::<3>::try_new(capacity::Count::new(3)).expect("count fits bound"),
        )
    });
    assert_eq!(counts[0], 1);
    let mut cell = (cell).expect("the three-entry lane slab must allocate");

    let (_, counts) = Tracker::measure(|| {
        let vacant = (slab::Batch::new(&mut cell).map(|batch| batch.into_entry(READY)))
            .expect("the empty slab has a vacancy");
        let key = vacant.insert(1).key();
        let read = (cell.try_get(key)).expect("queue handle must retain its original generation");

        assert_eq!(*read, 1);
        let entry =
            (cell.try_entry(key)).expect("queue handle must retain its original generation");

        assert_eq!(entry.remove(), 1);
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn generation_routes_value() {
    let mut cell = (slab::Slab::<usize, Tag, { u32::MAX }, tagged::Hook, 2, _>::try_from(
        Count::<3>::try_new(capacity::Count::new(3)).expect("count fits bound"),
    ))
    .expect("the three-entry lane slab must allocate");
    let vacant = (slab::Batch::new(&mut cell).map(|batch| batch.into_entry(READY)))
        .expect("the first slot is vacant");
    let first = vacant.insert(10).key();
    let vacant = (slab::Batch::new(&mut cell).map(|batch| batch.into_entry(READY)))
        .expect("the second slot is vacant");
    let second = vacant.insert(20).key();

    assert_eq!(cell.len(), 2);
    assert_eq!(cell.available(), 1);
    assert!(!cell.is_empty());
    assert!(!cell.is_full());
    let read = (cell.try_get(second)).expect("queue handle must retain its original generation");

    assert_eq!(*read, 20);
    assert!(cell.front_pair(READY) == Some((first, &10)));

    {
        let ((), entry) = (cell.select([(READY, ())])).expect("the ready lane has a front entry");
        assert_eq!(*entry.as_ref(), 10);
        assert!(entry.key() == first);
        assert_eq!(entry.remove(), 10);
    }

    let ((), entry) = (cell.select([(READY, ())])).expect("the second entry remains ready");
    assert!(entry.key() == second);
    assert_eq!(entry.remove(), 20);
    assert!(cell.is_empty());
}

#[test]
fn removal_retires_key() {
    let mut cell = (slab::Slab::<usize, Tag, { u32::MAX }, tagged::Hook, 2, _>::try_from(
        Count::<2>::try_new(capacity::Count::new(2)).expect("count fits bound"),
    ))
    .expect("the two-entry lane slab must allocate");
    let vacant = (slab::Batch::new(&mut cell).map(|batch| batch.into_entry(READY)))
        .expect("the initial slot is vacant");
    let stale = vacant.insert(7).key();
    let vacant = (slab::Batch::new(&mut cell).map(|batch| batch.into_entry(READY)))
        .expect("the second slot is vacant");
    let next = vacant.insert(8).key();
    let entry = (cell.try_entry(stale)).expect("queue handle must retain its original generation");

    assert_eq!(entry.remove(), 7);

    assert!(cell.front_pair(READY) == Some((next, &8)));
    assert!(matches!(cell.try_entry(stale), Err(slab::Error::Free { key, .. }) if key == stale));
    assert!(matches!(
        cell.try_get(stale),
        Err(slab::Error::Free { key, actual })
            if key == stale && Some(actual) == stale.generation().checked_add(1)
    ));

    let vacant = (slab::Batch::new(&mut cell).map(|batch| batch.into_entry(READY)))
        .expect("the removed slot is reusable");
    let current = vacant.insert(9).key();
    assert_eq!(current.index(), stale.index());
    assert!(current != stale);
    assert!(matches!(
        cell.try_get(stale),
        Err(slab::Error::Generation { key, actual })
            if key == stale && actual == current.generation()
    ));
    let entry =
        (cell.try_entry(current)).expect("queue handle must retain its original generation");

    assert_eq!(*entry.as_ref(), 9);
}

#[test]
fn classifies_index() {
    let mut source =
        slab::Slab::<usize, Tag>::try_from(capacity::Count::new(2)).expect("source allocation");
    assert_eq!(*source.vacant().expect("first slot").insert(1).as_ref(), 1);
    let key = source.vacant().expect("second slot").insert(2).key();

    for count in 0..=2 {
        let capacity = capacity::Count::new(count);
        let mut target = slab::Slab::<usize, Tag>::try_from(capacity).expect("target allocation");
        assert!(target.entry(key).is_none());
        if count <= key.index() {
            assert!(matches!(target.try_entry(key),
                Err(slab::Error::Index { key: actual, capacity: bound })
                    if actual == key && bound == capacity));
        } else {
            assert!(matches!(target.try_entry(key),
                Err(slab::Error::Uninitialized { key: actual, initialized: 0 })
                    if actual == key));
            assert_eq!(
                *target
                    .vacant()
                    .expect("first target slot")
                    .insert(3)
                    .as_ref(),
                3
            );
            assert!(matches!(target.try_get(key),
                Err(slab::Error::Uninitialized { key: actual, initialized: 1 })
                    if actual == key));
        }
    }
}

#[test]
fn empty_allocates_nothing() {
    let (built, counts) = Tracker::measure(|| {
        (
            slab::Slab::<usize, Tag>::try_from(capacity::Count::new(0)),
            slab::Slab::<usize, Tag, { u32::MAX }, tagged::Hook, 0, capacity::Count>::try_from(
                capacity::Count::new(0),
            ),
        )
    });
    assert_eq!(counts, [0; 4]);
    let (fifo, fixed) = match built {
        (Ok(fifo), Ok(fixed)) => Some((fifo, fixed)),
        _ => None,
    }
    .expect("all zero-capacity lane stores must construct without allocation");
    assert_eq!(fifo.available(), 0);
    assert_eq!(fixed.available(), 0);
}

#[test]
fn edits_owned_slot() {
    let mut fifo = (slab::Slab::<usize, Tag, 1>::try_from(capacity::Count::new(2)))
        .expect("the two-slot FIFO must allocate");
    let first =
        (fifo.vacant().map(|slot| slot.insert(1).key())).expect("the first vacancy must exist");
    let second =
        (fifo.vacant().map(|slot| slot.insert(2).key())).expect("the second vacancy must exist");
    let (result, hot) = Tracker::measure(|| {
        let address = {
            let mut entry = fifo.try_entry(first)?;
            let address = entry.as_ref() as *const usize;
            *entry.as_mut() = 3;
            address
        };
        assert!(fifo.front_pair() == Some((first, &3)));
        let read = fifo.try_get(first)?;
        assert_eq!(read as *const usize, address);
        assert_eq!(*read, 3);
        assert_eq!(fifo.available(), 0);
        assert_eq!(fifo.pop_front(), Some(3));
        assert!(fifo.front_pair() == Some((second, &2)));
        assert_eq!(fifo.pop_front(), Some(2));
        assert!(
            matches!(fifo.try_entry(first), Err(slab::Error::Retired { key, .. }) if key == first)
        );
        assert_eq!(fifo.available(), 0);
        Ok::<(), slab::Error<Tag, 1>>(())
    });
    result.as_ref().expect("required fixture outcome");
    assert_eq!(hot, [0; 4]);
}

#[test]
fn recycles_after_remove() {
    let mut queue = (slab::Slab::<String, Domain>::try_from(capacity::Count::new(3)))
        .expect("the three-slot generational FIFO must allocate");
    let first = (queue
        .vacant()
        .map(|vacant| vacant.insert(String::from("first")).key()))
    .expect("the first value fits");
    let second = (queue
        .vacant()
        .map(|vacant| vacant.insert(String::from("second")).key()))
    .expect("the second value fits");
    let third = (queue
        .vacant()
        .map(|vacant| vacant.insert(String::from("third")).key()))
    .expect("the third value fits");

    assert_eq!(queue.available(), 0);
    assert!(queue.vacant().is_none());
    let (front, value) = queue
        .front_pair()
        .expect("the generational FIFO has a front");
    assert_eq!(front.index(), first.index());
    assert!((front.generation()) == (first.generation()));
    assert_eq!(value, "first");

    queue
        .entry(second)
        .expect("the second handle remains occupied")
        .as_mut()
        .push_str("-updated");
    assert_eq!(
        queue
            .entry(first)
            .expect("the first handle remains occupied")
            .remove()
            .as_str(),
        "first"
    );
    assert!(queue.entry(first).is_none());
    let reused = (queue
        .vacant()
        .map(|vacant| vacant.insert(String::from("reused")).key()))
    .expect("the removed slot is reusable");
    assert_eq!(reused.index(), first.index());
    assert!((reused.generation()) != (first.generation()));

    for (key, expected) in [
        (second, "second-updated"),
        (third, "third"),
        (reused, "reused"),
    ] {
        assert!(queue.front_pair().map(|(front, _)| front) == Some(key));
        assert_eq!(queue.pop_front().as_deref(), Some(expected));
    }
    assert!(queue.is_empty());
    assert_eq!(queue.available(), 3);
}

#[test]
fn matches_deque_model() {
    const CAPACITY: usize = 17;
    let mut queue = (slab::Slab::<u64, Domain>::try_from(capacity::Count::new(CAPACITY as u32)))
        .expect("the seventeen-slot model FIFO must allocate");
    let mut model = VecDeque::new();
    let mut rng = Lcg::<1_442_695_040_888_963_407>::new(0x9e37_79b9_7f4a_7c15u64);

    for step in 0..20_000u64 {
        let seed = rng.advance();
        match (seed >> 61) as u8 {
            0..=2 => {
                let value = seed ^ step.rotate_left(17);
                match queue.vacant() {
                    Some(vacant) => {
                        let slot = vacant.insert(value).key();
                        assert!(model.len() < CAPACITY);
                        assert!(!model.iter().any(|(current, _)| *current == slot));
                        model.push_back((slot, value));
                    }
                    None => {
                        assert_eq!(model.len(), CAPACITY);
                    }
                }
            }
            3 | 4 => {
                let head = queue.front_pair().map(|(key, _)| key);
                let actual = queue.pop_front();
                let expected = model.pop_front();
                assert!(head == expected.as_ref().map(|(key, _)| *key));
                assert_eq!(actual, expected.map(|(_, value)| value));
            }
            5 if !model.is_empty() => {
                let pos = seed as usize % model.len();
                let (slot, value) = model.remove(pos).expect("the model position exists");
                assert_eq!(
                    queue
                        .entry(slot)
                        .expect("the modeled generational slot is occupied")
                        .remove(),
                    value
                );
                assert!(queue.entry(slot).is_none());
            }
            6 if !model.is_empty() => {
                let pos = seed as usize % model.len();
                let (slot, value) = &mut model[pos];
                *value = value.rotate_left(7);
                *queue
                    .entry(*slot)
                    .expect("the modeled generational slot is occupied")
                    .as_mut() = *value;
            }
            _ => {
                let actual = queue.front_pair();
                assert!(actual == model.front().map(|(key, value)| (*key, value)));
            }
        }
        assert_eq!(queue.is_empty(), model.is_empty());
        assert_eq!(queue.available(), CAPACITY - model.len());
    }

    while let Some((key, value)) = model.pop_front() {
        assert!(queue.front_pair().map(|(head, _)| head) == Some(key));
        assert_eq!(queue.pop_front(), Some(value));
    }
    assert!(queue.is_empty());
}
