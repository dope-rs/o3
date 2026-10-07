pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, collections::VecDeque, mem, rc::Rc};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{
        index::{Zero, capacity::Single},
        model::Random,
        queue::indexed::Model,
    },
};
use o3::depot::{
    self,
    capacity::{self, Count},
    queue::indexed::fifo::{
        self,
        staged::{self, Entry, State},
    },
};

#[test]
fn keeps_key_and_order() {
    let result = staged::Table::<u64, String, (String, u64)>::try_new(4);
    let mut table = result.expect("the four-slot ordered table must allocate");

    let Entry::Vacant(entry) = table.entry(2).expect("fixture slot must exist") else {
        panic!("the second fixture index must be vacant");
    };
    entry.insert(22, String::from("second"));
    let entry = table
        .vacant(0)
        .expect("the first fixture index must be vacant");
    entry.insert(10, String::from("first"));

    let front = table.front().expect("queued front");
    assert_eq!(
        (front.index(), *front.key(), front.as_ref().as_str()),
        (2, 22, "second")
    );

    let Entry::Queued(mut entry) = table.entry(2).expect("fixture slot must exist") else {
        panic!("the second fixture slot must be queued");
    };
    let value = mem::take(entry.as_mut());
    assert_eq!(*entry.key(), 22);
    let queued = entry.set_parked((value, 7));
    assert!(queued.is_empty());
    assert!(matches!(
        table.get(2),
        Some(State::Parked { key }) if *key == 22
    ));
    let (key, parked) = table.parked(2).expect("the second slot is parked");
    assert_eq!((*key, parked.0.as_str(), parked.1), (22, "second", 7));
    assert_eq!(table.front().map(|entry| entry.index()), Some(0));

    let Entry::Parked(entry) = table.entry(2).expect("fixture slot must exist") else {
        panic!("the second fixture slot must be parked");
    };
    let parked = entry.take();
    assert_eq!(parked, (22, (String::from("second"), 7)));
    let Entry::Queued(entry) = table.entry(0).expect("fixture slot must exist") else {
        panic!("the first fixture slot must be queued");
    };
    let queued = entry.take();
    assert_eq!(queued, (10, String::from("first")));
    assert!(table.is_empty());
}

#[test]
fn parks_before_return() {
    let result = staged::Table::<u8, String, usize>::try_new(1);
    let mut table = result.expect("the one-slot parking table must allocate");
    let entry = table
        .vacant(0)
        .expect("the park fixture slot must be vacant");
    entry.insert(9, String::from("queued"));
    let Entry::Queued(entry) = table.entry(0).expect("fixture slot must exist") else {
        panic!("the park fixture slot must be queued");
    };
    let value = entry.as_ref().len();
    assert_eq!(*entry.key(), 9);
    let (entry, queued) = entry.park(value);
    assert_eq!(queued, "queued");
    let gone = entry.take();
    assert_eq!(gone, (9, 6));
}

#[test]
fn reuse_matches_model() {
    const CAPACITY: usize = 31;
    const STEPS: usize = 100_000;

    let result = staged::Table::<u64, u64, u64>::try_new(CAPACITY);
    let mut table = result.expect("the 31-slot reuse model must allocate");
    let mut order = VecDeque::new();
    let mut slots = [None; CAPACITY];
    let mut rng = Random::<u64, 7, 9, 8>::try_new(0xd1b5_4a32_d192_ed03_u64).expect("shifts");

    for epoch in 1..=STEPS as u64 {
        let random = rng.advance();
        let raw = (random % CAPACITY as u64) as u32;
        let index = raw as usize;

        match (random >> 16) % 5 {
            0 if slots[index].is_none() => {
                let value = epoch << 8 | u64::from(raw);
                let entry = table
                    .vacant(raw)
                    .expect("the model proves this fixture index vacant");
                entry.insert(epoch, value);
                slots[index] = Some(Model {
                    parked: false,
                    key: epoch,
                    value,
                });
                order.push_back(raw);
            }
            1 if slots[index].is_some_and(|model| !model.parked) => {
                let model = slots[index].expect("queued model guard");
                let Entry::Queued(entry) = table.entry(raw).expect("fixture slot must exist")
                else {
                    panic!("the model proves this fixture index queued");
                };
                assert_eq!(*entry.key(), model.key);
                let queued = entry.set_parked(!model.value);
                assert_eq!(queued, model.value);
                slots[index] = Some(Model {
                    parked: true,
                    key: model.key,
                    value: !model.value,
                });
                let pos = (order.iter().position(|slot| *slot == raw))
                    .expect("queued slot in order model");
                order.remove(pos);
            }
            2 if slots[index].is_some_and(|model| model.parked) => {
                let model = slots[index].take().expect("parked model guard");
                let Entry::Parked(entry) = table.entry(raw).expect("fixture slot must exist")
                else {
                    panic!("the model proves this fixture index parked");
                };
                let gone = entry.take();
                assert_eq!(gone, (model.key, model.value));
            }
            3 if slots[index].is_some_and(|model| !model.parked) => {
                let model = slots[index].take().expect("queued model guard");
                let Entry::Queued(entry) = table.entry(raw).expect("fixture slot must exist")
                else {
                    panic!("the model proves this removable fixture index queued");
                };
                let gone = entry.take();
                assert_eq!(gone, (model.key, model.value));
                let pos = (order.iter().position(|slot| *slot == raw))
                    .expect("removed queued slot in order model");
                order.remove(pos);
            }
            4 if slots[index].is_some_and(|model| model.parked) => {
                let model = slots[index].expect("parked model guard");
                let Entry::Parked(entry) = table.entry(raw).expect("fixture slot must exist")
                else {
                    panic!("the model proves this fixture index parked");
                };
                let (entry, parked) = entry.resume(!model.value);
                assert_eq!(
                    (*entry.key(), *entry.as_ref(), parked),
                    (model.key, !model.value, model.value)
                );
                slots[index] = Some(Model {
                    parked: false,
                    key: model.key,
                    value: !model.value,
                });
                order.push_back(raw);
            }
            _ => {}
        }

        assert_eq!(
            table.front().map(|entry| entry.index()),
            order.front().copied()
        );
        assert_eq!(table.len(), slots.iter().flatten().count());
        for (index, want) in slots.iter().enumerate() {
            let raw = index as u32;
            let actual = table.get(raw).expect("every model index is in bounds");
            let actual = match actual {
                State::Vacant => None,
                State::Queued { key } => Some((false, *key)),
                State::Parked { key } => Some((true, *key)),
            };
            assert_eq!(actual, want.map(|model| (model.parked, model.key)));
        }
    }
}

#[test]
fn drops_live_once() {
    let drops = Rc::new(Cell::new(Some(0)));
    {
        let result = staged::Table::<
            Counted<Rc<Cell<Option<usize>>>>,
            Option<Counted<Rc<Cell<Option<usize>>>>>,
            Counted<Rc<Cell<Option<usize>>>>,
        >::try_new(3);
        let mut table = result.expect("the three-slot drop table must allocate");
        for index in 0..3 {
            let entry = table
                .vacant(index)
                .expect("each drop fixture index must initially be vacant");
            entry.insert(Counted(Rc::clone(&drops)), Some(Counted(Rc::clone(&drops))));
        }
        let Entry::Queued(mut entry) = table.entry(1).expect("fixture slot must exist") else {
            panic!("the second drop fixture index must be queued");
        };
        let value = entry
            .as_mut()
            .take()
            .expect("the second queued fixture payload must be present");
        let queued = entry.set_parked(value);
        assert!(queued.is_none());
        let Entry::Queued(entry) = table.entry(0).expect("fixture slot must exist") else {
            panic!("the first drop fixture index must be queued");
        };
        {
            let gone = entry.take();
            assert!(gone.1.is_some());
        }
        assert_eq!(drops.get(), Some(2));
    }
    assert_eq!(drops.get(), Some(6));
}

#[test]
fn clears_then_reuses() {
    for capacity in [3, 8] {
        let drops = Cell::new(Some(0));
        {
            let mut table = (staged::Table::<
                Counted<&Cell<Option<usize>>>,
                Counted<&Cell<Option<usize>>>,
                Counted<&Cell<Option<usize>>>,
            >::try_new(capacity))
            .expect("the sparse cleanup fixture must allocate");
            for index in [0, 2] {
                let entry = table
                    .vacant(index)
                    .expect("the initial sparse slot must be vacant");
                entry.insert(Counted(&drops), Counted(&drops));
            }
            let Entry::Queued(entry) = table.entry(2).expect("fixture slot must exist") else {
                panic!("the second sparse slot must be queued");
            };
            {
                let previous = entry.set_parked(Counted(&drops));
                assert_eq!(previous.0.get(), Some(0));
            }
            assert_eq!(drops.get(), Some(1));

            table.clear();
            assert_eq!(drops.get(), Some(5));
            assert!(table.is_empty());
            assert!(table.front().is_none());
            for index in 0..capacity as u32 {
                assert!(matches!(table.get(index), Some(State::Vacant)));
            }
            table.clear();
            assert_eq!(drops.get(), Some(5));

            let entry = table.vacant(1).expect("a cleared table must admit reuse");
            entry.insert(Counted(&drops), Counted(&drops));
        }
        assert_eq!(drops.get(), Some(7));
    }
}

#[test]
fn layout_is_compact() {
    let word = mem::size_of::<usize>();
    assert_eq!(
        staged::Table::<u64, [u64; 2], [u64; 5]>::SLOT_BYTES,
        8 * word
    );
    assert_eq!(
        mem::size_of::<staged::Slot<'static, u64, u64, [u64; 5], ()>>(),
        2 * word
    );
    assert_eq!(
        mem::size_of::<staged::Slot<'static, u64, u64, [u64; 5], staged::Queued>>(),
        2 * word
    );
    assert_eq!(
        mem::size_of::<staged::Slot<'static, u64, u64, [u64; 5], staged::Parked>>(),
        2 * word
    );
    assert_eq!(
        mem::size_of::<Option<staged::Slot<'static, u64, u64, [u64; 5], ()>>>(),
        2 * word
    );
    assert_eq!(
        mem::size_of::<Option<staged::Slot<'static, u64, u64, [u64; 5], staged::Queued>>>(),
        2 * word
    );
    assert_eq!(
        mem::size_of::<Option<staged::Slot<'static, u64, u64, [u64; 5], staged::Parked>>>(),
        2 * word
    );
}

#[test]
fn allocations_are_exact() {
    const CAPACITY: usize = 257;
    const SLOT_BYTES: usize = staged::Table::<u64, u64, [u64; 5]>::SLOT_BYTES;

    let count = capacity::Count::new(CAPACITY as u32);
    let (result, alloc) =
        Tracker::measure(|| staged::Table::<u64, u64, [u64; 5]>::try_count(count));
    let mut table = result.expect("the 257-slot allocation table must allocate");
    assert!((table.capacity()) == (count));
    assert_eq!(alloc, [1, CAPACITY * SLOT_BYTES, 0, 0]);

    let ((), alloc) = Tracker::measure(|| {
        for index in 0..CAPACITY as u32 {
            let entry = table
                .vacant(index)
                .expect("each allocation fixture index must initially be vacant");
            entry.insert(u64::from(index) + 1, u64::from(index));
        }
        for index in 0..CAPACITY as u32 {
            let Entry::Queued(entry) = table.entry(index).expect("fixture slot must exist") else {
                panic!("each inserted allocation fixture index must be queued");
            };
            let value = *entry.as_ref();
            let queued = entry.set_parked([value; 5]);
            assert_eq!(queued, value);
        }
        for index in 0..CAPACITY as u32 {
            let Entry::Parked(entry) = table.entry(index).expect("fixture slot must exist") else {
                panic!("each transitioned allocation fixture index must be parked");
            };
            let (key, value) = entry.take();
            assert_eq!((key, value), (u64::from(index) + 1, [u64::from(index); 5]));
        }
    });
    assert_eq!(alloc, [0, 0, 0, 0]);

    let ((), alloc) = Tracker::measure(move || assert!(table.is_empty()));
    assert_eq!(alloc, [0, 0, 1, CAPACITY * SLOT_BYTES]);
}

#[test]
fn keeps_singleton_domain() {
    assert!(matches!(
        fifo::List::<u8, Zero>::try_new(2),
        Err(o3::alloc::Error::Overflow)
    ));
    for queue in [
        fifo::List::<u8, Zero>::try_count(Single),
        fifo::List::try_new(1),
    ] {
        let mut queue = queue.expect("singleton domain");
        queue.vacant_entry(Zero).expect("vacant slot").push_back(7);
        assert_eq!(queue.pop_front(), Some(7));
        assert!(queue.is_empty());
    }
    let ((), allocs) = Tracker::measure(|| {
        let empty = staged::Table::<(), (), (), Zero>::try_new(0).expect("empty domain");
        assert!(empty.is_empty());
        for capacity in [2, usize::MAX] {
            assert!(matches!(
                staged::Table::<(), (), (), Zero>::try_new(capacity),
                Err(o3::alloc::Error::Overflow)
            ));
        }
    });
    assert_eq!(allocs, [0; 4]);
}

#[test]
fn keeps_fifo_order() {
    let mut queue =
        (fifo::List::<&str>::try_new(4)).expect("the four-slot indexed FIFO must allocate");
    let one = (queue.vacant_entry(1)).expect("index one must be vacant");
    one.push_back("one");
    assert!(queue.vacant_entry(1).is_none());
    assert!(queue.vacant_entry(4).is_none());

    let zero = (queue.vacant_entry(0)).expect("index zero must be vacant");
    zero.push_back("zero");
    assert_eq!(queue.capacity(), 4);
    assert_eq!(queue.pop_front(), Some("one"));
    assert_eq!(queue.pop_front(), Some("zero"));
    assert!(queue.is_empty());

    let first = (queue.vacant_entry(0)).expect("index zero must be vacant again");
    first.push_back("first");
    let second = (queue.vacant_entry(1)).expect("index one must be vacant again");
    second.push_back("second");
    let fifo::Entry::Occupied(entry) = queue.entry(0).expect("index zero must remain occupied")
    else {
        panic!("index zero must remain occupied");
    };
    assert_eq!(entry.remove().1, "first");
    assert_eq!(queue.pop_front(), Some("second"));
}

#[test]
fn moves_replace_to_back() {
    let mut queue =
        (fifo::List::<&str>::try_new(2)).expect("the two-slot indexed FIFO must allocate");
    let old = (queue.vacant_entry(0)).expect("index zero must be vacant");
    old.push_back("old");
    let other = (queue.vacant_entry(1)).expect("index one must be vacant");
    other.push_back("other");

    let fifo::Entry::Occupied(entry) = queue.entry(0).expect("index zero must be occupied") else {
        panic!("index zero must be occupied");
    };
    assert_eq!(entry.get(), &"old");
    let (vacant, old) = entry.remove();
    assert_eq!(old, "old");
    vacant.push_back("new");

    assert_eq!(queue.pop_front(), Some("other"));
    assert_eq!(queue.pop_front(), Some("new"));
    assert!(queue.entry(2).is_none());
}

#[test]
fn indexed_allocates_once() {
    let (created, alloc) = Tracker::measure(|| fifo::List::<[usize; 2]>::try_count(Count::new(4)));
    assert_eq!(alloc, [1, 96, 0, 0]);
    let mut queue = (created).expect("the measured four-slot indexed queue must allocate once");

    let ((), hot) = Tracker::measure(|| {
        let slot = (queue.vacant_entry(2)).expect("index two must be vacant");
        slot.push_back([1, 0]);
        let slot = (queue.vacant_entry(0)).expect("index zero must be vacant");
        slot.push_back([2, 0]);
        assert_eq!(queue.pop_front(), Some([1, 0]));
        let Some(fifo::Entry::Occupied(entry)) = queue.entry(0) else {
            panic!("index zero must remain occupied");
        };
        let removed = entry.remove();
        assert_eq!(removed.1, [2, 0]);
    });
    assert_eq!(hot, [0; 4]);

    let ((), drops) = Tracker::measure(|| {
        let released = queue;
        assert!(released.is_empty());
    });
    assert_eq!(drops, [0, 0, 1, 96]);
}

#[test]
fn retains_owner() {
    let created = staged::Table::<u32, u32, u32>::try_new(2);
    let owner = created.expect("fixture table must allocate");
    assert!(staged::Member::get(&owner, 2).is_none());

    let first = staged::Member::get(&owner, 1).expect("the first member must exist");
    let queued = staged::Member::get(&owner, 1).expect("the queued member must exist");
    let parked = staged::Member::get(&owner, 1).expect("the parked member must exist");
    assert!(!first.has_queued());
    let staged::Entry::Vacant(first) =
        staged::Entry::from_source(unsafe { depot::raw::Proof::new(first) })
    else {
        panic!("the retained position starts vacant");
    };
    first.insert(7, 11);

    assert!(queued.has_queued());
    assert!(
        staged::Member::get(&owner, 0)
            .expect("other vacant member")
            .has_queued()
    );
    let staged::Entry::Queued(mut queued) =
        staged::Entry::from_source(unsafe { depot::raw::Proof::new(queued) })
    else {
        panic!("the existing membership observes the inserted value");
    };
    assert_eq!((*queued.key(), *queued.as_ref()), (7, 11));
    *queued.as_mut() = 13;
    assert_eq!(queued.set_parked(17), 13);

    assert!(!parked.has_queued());
    let staged::Entry::Parked(parked) =
        staged::Entry::from_source(unsafe { depot::raw::Proof::new(parked) })
    else {
        panic!("the existing membership observes the parked value");
    };
    assert_eq!(parked.take(), (7, 17));
    assert!(owner.is_empty());
}

#[test]
fn retains_parked_deadline() {
    use o3::depot::{capacity, heap::indexed};

    for capacity in [1, 7, 64, 257] {
        let count = capacity::Count::new(capacity);
        let mut table = staged::Table::<u64, (), u64>::try_count(count).expect("table");
        let mut heap = (indexed::Min::<u32>::try_new(count))
            .expect("heap storage must fit its declared domain");

        let ((), counts) = Tracker::measure(|| {
            for epoch in 0..16 {
                for index in 0..capacity {
                    let staged::Entry::Vacant(entry) = table.entry(index).expect("bounded slot")
                    else {
                        panic!("reusable slot");
                    };
                    entry.insert(u64::from(index) + epoch * u64::from(capacity), ());
                }
                while let Some(queued) = table.front() {
                    let position = queued.position();
                    let live = unsafe { depot::raw::Proof::new(((&mut heap, queued), position)) };
                    let source = indexed::Slot::<_, _, _, indexed::Empty>::from_source(live);
                    let (deadline, queued) = source.entry();
                    let key = *queued.key();
                    let (parked, ()) = queued.park(!key);
                    assert_eq!(*parked.key(), key);
                    assert_eq!(
                        *deadline.insert(capacity - position.get()).priority(),
                        capacity - position.get()
                    );
                }
                for index in (0..capacity).rev() {
                    let deadline = heap.first_entry().expect("earliest deadline");
                    assert_eq!(deadline.index(), index as usize);
                    let position = deadline.position();
                    let (parked, deadline) =
                        staged::Slot::<_, _, _, staged::Parked, _>::from_source(unsafe {
                            depot::raw::Proof::new(((&mut table, deadline), position))
                        });
                    let key = u64::from(index) + epoch * u64::from(capacity);
                    assert_eq!(parked.take(), (key, !key));
                    deadline.retire();
                }
                assert!(table.is_empty());
                assert!(heap.is_empty());
            }
        });
        assert_eq!(counts, [0, 0, 0, 0]);
    }
}
