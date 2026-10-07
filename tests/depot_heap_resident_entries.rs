pub mod common;

use std::{cell::Cell, mem};

use common::{
    alloc::system::Tracker,
    depot::{
        heap::resident::CAPACITY_257_BYTES,
        index::key::Id,
        model::{Lcg, drops::Tracked},
    },
};
use o3::depot::{
    capacity::Count,
    heap::{
        self,
        resident::{self, Entry},
    },
    index::Position,
    raw::{self, Key as _},
};

type Heap = resident::Min<Id<'static>, u64, u64, u16>;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn allocation_is_exact() {
    if usize::BITS == 64 {
        assert_eq!(mem::size_of::<Heap>(), 32);
    }
    let bytes = CAPACITY_257_BYTES;
    let mut ptrs = Vec::with_capacity(257);
    let released = {
        let (heap, counts) = Tracker::measure(|| Heap::try_repeat(Count::new(257), 0));
        let mut heap = (heap).expect("heap storage must fit its declared domain");

        assert_eq!(counts, [1, bytes, 0, 0]);
        for index in 0..257 {
            let entry = into_vacant(
                heap.entry(Id::try_new(index, 1).expect("identity components fit"))
                    .expect("resident heap key must remain in its original domain"),
            )
            .expect("the initial resident must be vacant");
            ptrs.push(entry.resident() as *const u64);
        }

        let ((), counts) = Tracker::measure(|| {
            for index in 0..257 {
                let mut entry = into_vacant(
                    (heap.entry(Id::try_new(index, 1).expect("identity components fit")))
                        .expect("resident heap key must remain in its original domain"),
                )
                .expect("each in-bounds fixture key must start vacant");
                assert_eq!(*entry.resident(), 0);
                *entry.resident_mut() = index as u64 + 1;
                entry.insert((257 - index) as u64);
            }
            for (index, &ptr) in ptrs.iter().enumerate() {
                let heap::Probe::Occupied(entry) =
                    heap.probe(Id::try_new(index, 1).expect("identity components fit"))
                else {
                    panic!("occupied resident");
                };
                assert_eq!(entry.resident() as *const u64, ptr);
            }
            while let Some(entry) = heap.first_entry() {
                entry.retire();
            }
        });
        assert_eq!(counts, [0; 4]);

        Tracker::begin()
    };
    assert_eq!(
        released.finish(),
        common::alloc::system::Activity {
            deallocations: 1,
            deallocated_bytes: bytes,
            ..Default::default()
        }
    );
}

#[test]
fn minimum_change_is_exact() {
    let mut heap = Heap::try_repeat(Count::new(3), 0).expect("three resident slots");
    let first = Id::try_new(0, 1).expect("identity components fit");
    let second = Id::try_new(1, 1).expect("identity components fit");

    for (key, priority) in [(first, 20), (second, 30)] {
        let Entry::Vacant(entry) = heap.entry(key).expect("bounded key") else {
            panic!("vacant entry");
        };
        entry.insert(priority);
    }
    for change in [heap::Change::Minimum, heap::Change::Contents] {
        let heap::Probe::Occupied(entry) = heap.probe(second) else {
            panic!("occupied entry");
        };
        assert!(entry.set_min(10) == change);
    }
    for (key, change) in [
        (first, heap::Change::Contents),
        (second, heap::Change::Minimum),
    ] {
        let heap::Probe::Occupied(entry) = heap.probe(key) else {
            panic!("occupied entry");
        };
        assert!(entry.retire_min() == change);
    }
}

#[test]
fn preserves_generation() {
    let mut heap = Heap::try_repeat(Count::new(3), 0).expect("three resident heap slots");
    let old = Id::try_new(1, 7).expect("identity components fit");
    let next = Id::try_new(1, 8).expect("identity components fit");
    assert!(matches!(heap.probe(next), heap::Probe::Vacant));
    let Entry::Vacant(mut entry) = heap.entry(old).expect("bounded old generation") else {
        panic!("vacant entry");
    };
    *entry.resident_mut() = 41;
    entry.insert(40);
    assert!(matches!(
        heap.probe(Id::try_new(3, 7).expect("identity components fit")),
        heap::Probe::OutOfRange
    ));
    let Entry::Conflict(mut entry) = heap.entry(next).expect("bounded next generation") else {
        panic!("conflicting generation");
    };
    assert_eq!(entry.current(), old);
    assert_eq!(entry.requested(), next);
    assert_eq!(*entry.resident_mut(), 41);
    *entry.resident_mut() = 42;
    entry.set(3);
    assert!(matches!(heap.probe(old), heap::Probe::Conflict));
    assert!(heap.peek() == Some((next, &3)));
    assert!(matches!(heap.probe(next), heap::Probe::Occupied(entry)
        if entry.key() == next && *entry.resident() == 42));
    assert!(
        heap.first_entry().map(|entry| {
            let gone = entry.remove();
            (gone.key(), *gone.priority())
        }) == Some((next, 3))
    );
    assert!(matches!(heap.probe(old), heap::Probe::Vacant));
}

#[test]
fn churn_matches_model() {
    const CAPACITY: usize = 47;
    let heap = Heap::try_repeat(Count::new(CAPACITY as u32), 0);
    let mut heap = (heap).expect("heap storage must fit its declared domain");

    let mut ptrs = Vec::with_capacity(CAPACITY);
    for index in 0..CAPACITY {
        let entry = into_vacant(
            heap.entry(Id::try_new(index, 1).expect("identity components fit"))
                .expect("resident heap key must remain in its original domain"),
        )
        .expect("the initial resident must be vacant");
        ptrs.push(entry.resident() as *const u64);
    }
    let mut model = [None; CAPACITY];
    let mut epoch = 1u64;
    let mut rng = Lcg::<1_442_695_040_888_963_407>::new(0x517c_c1b7_2722_0a95u64);

    for _ in 0..50_000 {
        let random = rng.advance();
        let index = random as usize % CAPACITY;
        match random % 6 {
            0 => {
                epoch += 1;
                let key = Id::try_new(index, epoch).expect("identity components fit");
                let prio = random >> 32;
                let entry = (heap.entry(key))
                    .expect("resident heap key must remain in its original domain");

                match entry {
                    Entry::Vacant(entry) => entry.insert(prio),
                    Entry::Conflict(entry) => {
                        entry.set(prio);
                    }
                    Entry::Occupied(entry) => {
                        entry.set(prio);
                    }
                }
                model[index] = Some((key, prio));
            }
            1 => {
                if let Some((key, prio)) = model[index] {
                    let next = prio ^ random.rotate_left(13);
                    let (entry,) = match (heap.entry(key))
                        .expect("resident heap key must remain in its original domain")
                    {
                        Entry::Occupied(entry) => Some((entry,)),
                        _ => None,
                    }
                    .expect("a modeled resident must remain occupied under its exact key");
                    assert_eq!(*entry.priority(), prio);
                    entry.set(next);
                    model[index] = Some((key, next));
                }
            }
            2 => {
                if let Some((key, prio)) = model[index].take() {
                    let (entry,) = match heap.probe(key) {
                        heap::Probe::Occupied(entry) => Some((entry,)),
                        _ => None,
                    }
                    .expect("the modeled key must remain occupied");
                    assert_eq!(*entry.remove().priority(), prio);
                }
            }
            3 => match heap
                .entry(Id::try_new(index, epoch).expect("identity components fit"))
                .expect("resident heap key must remain in its original domain")
            {
                Entry::Vacant(mut entry) => *entry.resident_mut() = random,
                Entry::Occupied(mut entry) => *entry.resident_mut() = random,
                Entry::Conflict(mut entry) => *entry.resident_mut() = random,
            },
            4 => {
                let want = model
                    .iter()
                    .flatten()
                    .copied()
                    .min_by_key(|(key, prio)| (*prio, key.index()));
                assert!(heap.peek().map(|(key, p)| (key, *p)) == want);
            }
            _ => {
                let want = model
                    .iter()
                    .enumerate()
                    .filter_map(|(i, entry)| entry.map(|(key, p)| (i, key, p)))
                    .min_by_key(|(index, key, prio)| (*prio, *index, key.index()))
                    .filter(|(_, _, priority)| *priority <= random);
                assert!(
                    heap.first_entry_if(|priority| *priority <= random)
                        .map(|entry| {
                            let gone = entry.remove();
                            (gone.key(), *gone.priority())
                        })
                        == want.map(|(_, key, p)| (key, p))
                );
                if let Some((index, _, _)) = want {
                    model[index] = None;
                }
            }
        }
        assert_eq!(heap.len(), model.iter().flatten().count());
        let address = match heap
            .entry(Id::try_new(index, epoch).expect("identity components fit"))
            .expect("resident heap key must remain in its original domain")
        {
            Entry::Vacant(entry) => entry.resident() as *const u64,
            Entry::Occupied(entry) => entry.resident() as *const u64,
            Entry::Conflict(mut entry) => entry.resident_mut() as *const u64,
        };
        assert_eq!(address, ptrs[index]);
    }
}

#[test]
fn layout_is_aligned() {
    use common::depot::heap::resident::Aligned;

    let heap = resident::Min::<Id<'static>, u64, Aligned>::try_repeat(Count::new(3), Aligned(7));
    let mut heap = (heap).expect("heap storage must fit its declared domain");

    for index in 0..3 {
        let entry = into_vacant(
            heap.entry(Id::try_new(index, 1).expect("identity components fit"))
                .expect("resident heap key must remain in its original domain"),
        )
        .expect("each initial aligned resident must be vacant");
        let item = entry.resident();
        assert_eq!((item as *const Aligned as usize) % 128, 0);
        assert_eq!(item.0, 7);
    }

    let (empty, counts) = Tracker::measure(|| {
        resident::Min::<Id<'static>, u64, Aligned>::try_repeat(Count::new(0), Aligned(0))
    });
    assert_eq!(counts, [0; 4]);
    let counts = {
        let mut empty = empty.expect("heap storage must fit its declared domain");

        assert!(matches!(
            empty.probe(Id::try_new(0, 1).expect("identity components fit")),
            heap::Probe::OutOfRange
        ));
        Tracker::begin()
    };
    assert_eq!(counts.finish(), common::alloc::system::Activity::default());
}

#[test]
fn position_bounds() {
    let maximum = Count::new(u32::from(u16::MAX));
    resident::Min::<Id<'static>, (), (), u16>::try_repeat(maximum, ())
        .as_ref()
        .expect("required fixture outcome");
    let count = Count::new(maximum.raw() + 1);
    let (want, maximum) = match (resident::Min::<Id<'static>, (), (), u16>::try_repeat(count, ()))
        .err()
        .expect("resident capacity must retain the exact position-width failure")
    {
        heap::Error::Capacity {
            requested: want,
            maximum,
        } => Some((want, maximum)),
        _ => None,
    }
    .expect("resident capacity must retain the exact position-width failure");
    assert!((want) == (count));
    assert_eq!(maximum.get(), usize::from(u16::MAX));
}

#[test]
fn drops_priorities_once() {
    let drops = [const { Cell::new(Some(0)) }; 2];
    {
        let heap = resident::Min::<Id<'static>, Tracked<'_>, u8, u16>::try_repeat(Count::new(2), 0);
        let mut heap = heap.expect("heap storage must fit its declared domain");

        for index in 0..2 {
            let entry = into_vacant(
                (heap.entry(Id::try_new(index, 1).expect("identity components fit")))
                    .expect("resident heap key must remain in its original domain"),
            )
            .expect("each drop-order key must start vacant");
            entry.insert(Tracked::try_new(index as u8, &drops).expect("priority counter"));
        }
        assert_eq!(drops.each_ref().map(Cell::get), [Some(0); 2]);
    }
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn removed_reinserts() {
    let heap = Heap::try_repeat(Count::new(1), 11);
    let mut heap = (heap).expect("heap storage must fit its declared domain");

    let key = Id::try_new(0, 7).expect("identity components fit");
    let entry = into_vacant(
        (heap.entry(key)).expect("resident heap key must remain in its original domain"),
    )
    .expect("the in-bounds reinsertion key must start vacant");
    entry.insert(9);

    let (entry,) = match heap.probe(key) {
        heap::Probe::Occupied(entry) => Some((entry,)),
        _ => None,
    }
    .expect("the inserted key must remain removable");
    let mut gone = entry.remove();
    assert!(gone.key() == key);
    assert_eq!(gone.priority(), &9);
    assert_eq!(gone.resident(), &11);
    *gone.resident_mut() = 13;
    gone.reinsert(4);

    assert!(heap.peek() == Some((key, &4)));
    assert!(matches!(heap.probe(key), heap::Probe::Occupied(entry)
        if *entry.resident() == 13));
}

fn into_vacant<K: raw::Address, P: Ord, R: Copy, I: Position>(
    entry: Entry<'_, K, P, R, I>,
) -> Option<resident::Vacant<'_, K, P, R, I>> {
    match entry {
        Entry::Vacant(entry) => Some(entry),
        _ => None,
    }
}
