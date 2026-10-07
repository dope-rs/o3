pub mod common;

use std::cell::Cell;

use common::depot::{
    heap::EntryExt as _,
    model::{Lcg, drops::Tracked},
};
use o3::depot::{
    capacity::Count,
    heap::indexed::{Entry, Min},
};

#[test]
fn keeps_callback_owners() {
    use common::depot::heap::observed::Priority;

    let calls = Cell::new(0);
    let drops = Cell::new(0);
    let released = Cell::new(0);
    let replacement_drops = Cell::new(0);
    let compare = || calls.set(calls.get() + 1);
    let release = || {
        drops.set(drops.get() + 1);
        released.set(calls.get());
    };
    let release_replacement = || {
        replacement_drops.set(replacement_drops.get() + 1);
        release();
    };
    let mut heap = Min::try_new(Count::new(8)).expect("eight priorities allocate");
    let original = [10, 30, 20, 40, 50, 25, 21];
    for (index, value) in original.into_iter().enumerate() {
        heap.try_entry(index)
            .expect("bounded index")
            .into_vacant()
            .expect("fresh slot")
            .set(Priority::new(value, &compare, &release));
    }
    calls.set(0);
    heap.try_entry(7)
        .expect("bounded index")
        .into_vacant()
        .expect("spare slot")
        .set(Priority::new(0, &compare, &release));
    assert!(calls.get() > 0);
    assert_eq!((heap.len(), drops.get()), (8, 0));

    calls.set(0);
    heap.try_entry(4)
        .expect("bounded index")
        .occupied()
        .expect("resident slot")
        .set(Priority::new(1, &compare, &release));
    assert!(calls.get() > 0);
    assert_eq!((drops.get(), released.get()), (1, calls.get()));

    calls.set(0);
    heap.try_entry(4)
        .expect("bounded index")
        .occupied()
        .expect("unchanged resident")
        .set(Priority::new(1, &compare, &release_replacement));
    assert_eq!((calls.get(), drops.get(), released.get()), (1, 2, 1));

    calls.set(0);
    let first = heap.first_entry().expect("inserted minimum");
    assert_eq!(first.index(), 7);
    let priority = first.take();
    assert_eq!((priority.value, drops.get()), (0, 2));
    assert!(calls.get() > 0);
    drop(priority);
    assert_eq!(drops.get(), 3);

    calls.set(0);
    heap.first_entry().expect("replacement minimum").retire();
    assert!(calls.get() > 0);
    assert_eq!((drops.get(), released.get()), (4, calls.get()));
    assert_eq!(replacement_drops.get(), 1);
    assert_eq!(
        heap.peek().map(|(index, priority)| (index, priority.value)),
        Some((0, 10))
    );
    assert_eq!(heap.len(), 6);
    assert!(heap.get(4).is_none());
    assert!(heap.get(7).is_none());
    drop(heap);
    assert_eq!(drops.get(), 10);
}

#[test]
fn drops_reverse_prefix() {
    use common::depot::heap::observed::Priority;

    let dropped = Cell::new(0);
    let order = &dropped;
    let compare = || assert_eq!(dropped.get(), 0);
    let release = [1, 2, 3].map(|digit| move || order.set(order.get() * 10 + digit));
    let mut heap = Min::try_new(Count::new(5)).expect("heap allocation");
    for (index, release) in release.iter().enumerate() {
        heap.try_entry(index)
            .expect("bounded index")
            .into_vacant()
            .expect("fresh slot")
            .set(Priority::new(index as u32, &compare, release));
    }
    drop(heap);
    assert_eq!(dropped.get(), 321);
}

#[test]
fn repeats_indexed_heap() {
    let mut heap =
        (Min::try_repeat(Count::new(4), 7_u64)).expect("heap storage must fit its declared domain");

    for index in 0..4 {
        assert_eq!(heap.get(index), Some(&7));
    }
    let mut seen = [false; 4];
    while let Some((index, priority)) = heap
        .first_entry()
        .map(|entry| (entry.index(), entry.take()))
    {
        assert_eq!(priority, 7);
        assert!(!seen[index]);
        seen[index] = true;
    }
    assert_eq!(seen, [true; 4]);
}

#[test]
fn matches_heap_model() {
    let mut rng = Lcg::<1>::new(1u64);
    let mut heap = Min::<(u32, usize)>::try_new(Count::new(64))
        .expect("heap storage must fit its declared domain");

    let mut model = [None; 64];
    assert_eq!(heap.get(0), None);

    let entry = heap
        .try_entry(0)
        .expect("heap index must remain in its original domain")
        .into_vacant()
        .expect("fresh heap index zero must be vacant");
    entry.set((9, 0));
    assert_eq!(heap.get(0), Some(&(9, 0)));
    let entry = heap
        .try_entry(1)
        .expect("heap index must remain in its original domain")
        .into_vacant()
        .expect("fresh heap index one must be vacant");
    entry.set((4, 1));
    assert_eq!(heap.peek(), Some((1, &(4, 1))));
    let entry = heap
        .try_entry(0)
        .expect("heap index must remain in its original domain")
        .occupied()
        .expect("inserted model index must be occupied");
    assert_eq!(entry.take(), (9, 0));
    assert_eq!(heap.get(0), None);
    let entry = heap
        .try_entry(1)
        .expect("heap index must remain in its original domain")
        .occupied()
        .expect("inserted model index must be occupied");
    assert_eq!(entry.take(), (4, 1));

    let steps = if cfg!(miri) { 500 } else { 10_000 };
    for _ in 0..steps {
        let state = rng.advance();
        let index = (state as usize >> 16) & 63;
        let key = ((state >> 32) as u32, index);
        match state & 3 {
            0 | 1 => {
                let entry =
                    (heap.try_entry(index)).expect("heap index must remain in its original domain");
                let result = match entry {
                    Entry::Vacant(entry) => {
                        entry.set(key);
                        Ok(())
                    }
                    Entry::Occupied(_) => Err(key),
                };
                if model[index].is_some() {
                    assert_eq!(result, Err(key));
                } else {
                    model[index] = Some(key);
                    assert_eq!(result, Ok(()));
                }
            }
            2 => {
                let entry = heap
                    .try_entry(index)
                    .expect("heap index must remain in its original domain");

                let removed = match entry {
                    Entry::Vacant(_) => None,
                    Entry::Occupied(entry) => Some(entry.take()),
                };
                assert_eq!(removed, model[index].take());
            }
            _ => {
                let want = model
                    .iter()
                    .enumerate()
                    .filter_map(|(index, key)| key.map(|key| (index, key)))
                    .min_by_key(|(_, key)| *key);
                assert_eq!(heap.peek().map(|(index, key)| (index, *key)), want);
            }
        }
    }
}

#[test]
fn sets_and_pops() {
    let mut heap =
        (Min::<u64>::try_new(Count::new(3))).expect("heap storage must fit its declared domain");

    let entry = heap
        .try_entry(0)
        .expect("heap index must remain in its original domain")
        .into_vacant()
        .expect("the first fresh heap index must be vacant");
    entry.set(9);
    let entry = heap
        .try_entry(2)
        .expect("heap index must remain in its original domain")
        .into_vacant()
        .expect("the last fresh heap index must be vacant");
    entry.set(7);
    assert_eq!(heap.peek(), Some((2, &7)));
    let entry = heap
        .try_entry(2)
        .expect("heap index must remain in its original domain")
        .occupied()
        .expect("the inserted last slot must be occupied");
    let (entry, old) = entry.replace(11);
    assert_eq!((entry.index(), *entry.priority(), old), (2, 11, 7));
    assert_eq!(heap.peek(), Some((0, &9)));
    let entry = heap
        .try_entry(0)
        .expect("heap index must remain in its original domain")
        .occupied()
        .expect("the first heap index must be occupied");
    let (entry, old) = entry.replace(4);
    assert_eq!((entry.index(), *entry.priority(), old), (0, 4, 9));
    {
        let entry = (heap.first_entry()).expect("two occupied slots must have a minimum");
        assert_eq!((entry.index(), *entry.priority()), (0, 4));
    }
    assert_eq!(heap.len(), 2);
    let entry = (heap.first_entry()).expect("releasing a minimum borrow must retain its entry");
    assert_eq!((entry.index(), entry.take()), (0, 4));
    assert_eq!(
        heap.first_entry()
            .map(|entry| (entry.index(), entry.take())),
        Some((2, 11))
    );
}

#[test]
fn keeps_heap_ownership() {
    let drops = [const { Cell::new(Some(0)) }; 4];
    {
        let mut heap = Min::<Tracked<'_>>::try_new(Count::new(3))
            .expect("heap storage must fit its declared domain");

        for (index, value) in [(0, 2), (1, 0), (2, 1)] {
            let entry = heap
                .try_entry(index)
                .expect("heap index must remain in its original domain")
                .into_vacant()
                .expect("each fresh ownership heap index must be vacant");
            entry.set(Tracked::try_new(value, &drops).expect("drop counter exists"));
        }
        assert_eq!(heap.peek().map(|(index, _)| index), Some(1));

        let entry = heap
            .try_entry(2)
            .expect("heap index must remain in its original domain")
            .occupied()
            .expect("the inserted drop-count slot must be occupied");
        entry.retire();
        assert_eq!(drops.each_ref().map(Cell::get), [0, 1, 0, 0].map(Some));
        assert_eq!(heap.peek().map(|(index, _)| index), Some(1));

        heap.clear();
        assert!(heap.is_empty());
        assert_eq!(drops.each_ref().map(Cell::get), [1, 1, 1, 0].map(Some));
        let entry = heap
            .try_entry(0)
            .expect("heap index must remain in its original domain")
            .into_vacant()
            .expect("clear must restore index zero vacancy");
        entry.set(Tracked::try_new(3, &drops).expect("drop counter exists"));
        assert_eq!(drops.each_ref().map(Cell::get), [1, 1, 1, 0].map(Some));
    }
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 4]);
}
