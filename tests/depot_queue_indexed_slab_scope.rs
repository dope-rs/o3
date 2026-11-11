use o3::depot::array::Index;
pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use common::{
    alloc::system::Tracker,
    depot::{
        index::capacity::Count,
        queue::indexed::slab::{Domain, Node, READY, WAITING},
    },
};
use o3::depot::{
    capacity,
    queue::{indexed::slab, tagged},
};

#[test]
fn links_reserved_slots() {
    const FIRST: Index<2> = Index::at::<0>();
    const SECOND: Index<2> = Index::at::<1>();
    let mut queue = (slab::Slab::<Node, (), { u32::MAX }, tagged::Hook, 2, _>::try_from(
        Count::<4>::try_new(capacity::Count::new(4)).expect("count fits bound"),
    ))
    .expect("four slots must allocate");
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(FIRST)))
        .expect("first slot fits");
    let old = slot
        .insert(Node {
            value: 1,
            next: None,
        })
        .key();
    let (result, activity) = Tracker::measure(|| {
        let slots = slab::Batch::new(&mut queue).expect("three remaining slots fit");
        let mut tails = [Some(old), None];
        slots.insert([FIRST, SECOND, FIRST], [2, 3, 4], |slot, lane, value| {
            let slot = slot.select(lane);
            let value = Node { value, next: None };
            let key = match *lane.get(&tails) {
                Some(previous) => {
                    let (key, previous) = slot
                        .append(previous)
                        .expect("the prior insertion remains in this queue")
                        .insert(value);
                    previous.next = Some(key);
                    key
                }
                None => slot.insert(value).key(),
            };
            *lane.get_mut(&mut tails) = Some(key);
        });
    });
    assert_eq!(result, ());
    assert_eq!(activity, [0; 4]);
    let first = (queue.try_get(old)).expect("queue handle must retain its original generation");

    let next = (first.next).expect("first group extends old tail");
    let next = (queue.try_get(next)).expect("queue handle must retain its original generation");

    assert_eq!(next.value, 2);
    let last = (next.next).expect("same group keeps its newly inserted predecessor");
    assert!((queue.try_get(last).map(|entry| entry.value)) == (Ok(4)));
    assert_eq!(
        queue.front_pair(SECOND).map(|(_, entry)| entry.value),
        Some(3)
    );
}

#[test]
fn returns_reservations() {
    let mut queue = slab::Slab::<usize, (), 3, tagged::Hook, 2, _>::try_from(Count::<3>::FULL)
        .expect("three queue slots must allocate");
    assert!(matches!(
        slab::Batch::<_, _, 2, 4, 3, _>::new(&mut queue),
        Err(capacity::Shortage {
            requested,
            available: 3
        }) if requested.get() == 4
    ));
    assert_eq!(queue.available(), 3);
    let ((), counts) = Tracker::measure(|| {
        assert!(slab::Batch::<_, _, 2, 3, 3, _>::new(&mut queue).is_ok());
        let mut indices = [None; 3];
        slab::Batch::new(&mut queue)
            .expect("three slots fit")
            .insert([0, 1, 2], [0; 3], |slot, available, value| {
                assert_eq!(slot.available(), available);
                let entry = slot.select(READY).insert(value);
                let index = entry.key().index();
                assert!(!indices.contains(&Some(index)));
                indices[available] = Some(index);
                entry.remove();
            });
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(queue.available(), 3);
    assert_eq!(queue.len(), 0);

    for value in 1..=3 {
        let batch = slab::Batch::new(&mut queue).expect("one queue slot remains");
        let entry = batch.into_entry(READY).insert(value);
        assert_eq!(*entry.as_ref(), value);
    }
    assert!(queue.is_full());
    let ((), counts) = Tracker::measure(|| {
        assert!(slab::Batch::<_, _, 2, 0, 3, _>::new(&mut queue).is_ok());
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.available(), 0);
    assert_eq!(queue.front_pair(READY).map(|pair| *pair.1), Some(1));
}

#[test]
fn restores_last_generation() {
    let mut queue = slab::Slab::<usize, (), 1, tagged::Hook, 2, _>::try_from(Count::<3>::FULL)
        .expect("three queue slots must allocate");
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(READY)))
        .expect("the empty queue has a slot");
    let key = slot.insert(7).key();
    for value in 8..12 {
        let entry =
            (queue.try_entry(key)).expect("queue handle must retain its original generation");

        let (previous, pending) = entry.take(READY);
        assert_eq!(previous + 1, value);
        let (_, counts) = Tracker::measure(|| pending.restore(value));
        assert_eq!(counts, [0; 4]);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.available(), 2);
        assert!((queue.front_pair(READY)) == (Some((key, &value))));
    }
    {
        let entry =
            (queue.try_entry(key)).expect("queue handle must retain its original generation");
        let taken = entry.take(READY);
        assert_eq!(taken.0, 11);
    }
    assert_eq!(queue.len(), 0);
    assert_eq!(queue.available(), 2);
    assert!(matches!(queue.try_get(key),
        Err(slab::Error::Retired { key: actual, .. }) if actual == key));
}

#[test]
fn retires_extracted_slot() {
    let mut queue = slab::Slab::<usize, (), 3, tagged::Hook, 2, _>::try_from(Count::<3>::FULL)
        .expect("three queue slots must allocate");
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(READY)))
        .expect("the empty queue has a slot");
    let entry = slot.insert(1);
    let key = entry.key();
    {
        let taken = entry.take(READY);
        assert_eq!(taken.0, 1);
    }
    assert_eq!(queue.available(), 3);
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(WAITING)))
        .expect("the abandoned extraction returns its slot");
    let current = slot.insert(2).key();
    assert_eq!(current.index(), key.index());
    assert!((Some(current.generation())) == (key.generation().checked_add(1)));
    assert!(matches!(queue.try_get(key),
        Err(slab::Error::Generation { actual, .. }) if actual == current.generation()));
}

#[test]
fn appends_with_predecessor() {
    let mut queue = slab::Slab::<usize, (), 3, tagged::Hook, 2, _>::try_from(Count::<3>::FULL)
        .expect("three queue slots must allocate");
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(READY)))
        .expect("the empty queue has a slot");
    let entry = slot.insert(10);
    let key = entry.key();
    assert_eq!(entry.remove(), 10);
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(READY)))
        .expect("the removed slot is available");
    assert!(matches!(slot.append(key),
        Err(slab::Error::Free { key: actual, .. }) if actual == key));
    assert_eq!(queue.len(), 0);
    assert_eq!(queue.available(), 3);
    assert!(queue.front_pair(READY).is_none());
    assert!(queue.front_pair(WAITING).is_none());
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(READY)))
        .expect("failed admission returns the slot");
    let key = slot.insert(10).key();
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(WAITING)))
        .expect("the predecessor leaves two slots");
    let append = (slot.append(key)).expect("queue handle must retain its original generation");

    let (inserted, counts) = Tracker::measure(|| {
        let (next, previous) = append.insert(20);
        assert!((next) != (key));
        *previous += 1;
        next
    });
    assert_eq!(counts, [0; 4]);
    assert!((queue.front_pair(READY)) == (Some((key, &11))));
    assert!((queue.front_pair(WAITING)) == (Some((inserted, &20))));
    assert!((queue.available()) == (1));
}

#[test]
fn selects_ordered_policy() {
    let mut queue = slab::Slab::<usize, (), 3, tagged::Hook, 2, _>::try_from(Count::<3>::FULL)
        .expect("three queue slots must allocate");
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(WAITING)))
        .expect("the empty queue has a slot");
    let waiting = slot.insert(20).key();
    let order = [
        (READY, String::from("ready")),
        (WAITING, String::from("waiting")),
    ];
    {
        let (selected, counts) = Tracker::measure(|| queue.select(order));
        assert_eq!(counts[0], 0);
        let (policy, entry) = (selected).expect("the second lane contains a value");
        assert_eq!(policy, "waiting");
        assert!((entry.key()) == (waiting));
    }
    let slot = (slab::Batch::new(&mut queue).map(|batch| batch.into_entry(READY)))
        .expect("the first lane has space");
    let ready = slot.insert(10).key();
    let (policy, entry) =
        (queue.select([(READY, 1), (WAITING, 2)])).expect("both lanes contain a value");
    assert_eq!(policy, 1);
    assert!((entry.key()) == (ready));
    assert_eq!(entry.remove(), 10);
    let (policy, entry) =
        (queue.select([(READY, 1), (WAITING, 2)])).expect("the second lane remains occupied");
    assert_eq!(policy, 2);
    assert_eq!(entry.remove(), 20);
    assert!(queue.select([(READY, 1), (WAITING, 2)]).is_none());
}

#[test]
fn owned_alloc_is_exact() {
    use o3::depot::queue::indexed::slab;

    const CAPACITY: usize = 257;
    const ALLOCATION_BYTES: usize = 5_160;

    {
        let (empty, alloc) =
            Tracker::measure(|| slab::Slab::<u64, Domain>::try_from(capacity::Count::new(0)));
        assert_eq!(alloc, [0, 0, 0, 0]);
        let empty = (empty).expect("zero owned FIFO capacity must be representable");
        assert_eq!(empty.available(), 0);
    }

    let (queue, alloc) = Tracker::measure(|| {
        slab::Slab::<u64, Domain>::try_from(capacity::Count::new(CAPACITY as u32))
    });
    assert_eq!(alloc, [1, ALLOCATION_BYTES, 0, 0]);
    let mut queue = (queue).expect("the owned FIFO must allocate");

    let ((), alloc) = Tracker::measure(|| {
        let mut slots = [None; 257];
        for (index, slot) in slots.iter_mut().enumerate() {
            let vacant = (queue.vacant()).expect("each modeled index has a vacancy");
            *slot = Some(vacant.insert(index as u64).key());
        }
        for (index, slot) in slots.into_iter().enumerate() {
            let slot = slot.expect("occupied slot");
            assert_eq!(
                queue.entry(slot).map(|entry| *entry.as_ref()),
                Some(index as u64)
            );
            if index & 1 == 0 {
                assert_eq!(
                    queue.entry(slot).expect("occupied owned slot").remove(),
                    index as u64
                );
            }
        }
        while queue.pop_front().is_some() {}
        assert!(queue.is_empty());
    });
    assert_eq!(alloc, [0, 0, 0, 0]);

    let ((), alloc) = Tracker::measure(move || {
        assert_eq!(queue.available(), CAPACITY);
        assert!(queue.is_empty());
    });
    assert_eq!(alloc, [0, 0, 1, ALLOCATION_BYTES]);
}
