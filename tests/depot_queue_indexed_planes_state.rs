pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell, rc::Rc};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{
        model::drops::Block,
        queue::{
            Sequence,
            indexed::{DropValue, Drops, Ordinal},
        },
    },
};
use o3::depot::queue::indexed::planes::{
    self, bound, groups,
    map::{State, Table},
};

#[test]
fn allocation_contract() {
    const CAPACITY: usize = 257;
    const ENTRY_BYTES: usize = 32;

    assert_eq!(std::mem::size_of::<planes::Set<u64, 3>>(), 64);
    assert_eq!(planes::Set::<u64, 3>::ENTRY_BYTES, ENTRY_BYTES);

    let ((), alloc) = Tracker::measure(|| {
        let result = planes::Set::<u64, 3>::try_seed(0, ());
        let empty = result.expect("the empty plane set must be allocation-free");
        assert!(empty.is_empty());
    });
    assert_eq!(alloc, [0, 0, 0, 0]);

    let (defaults, alloc) = Tracker::measure(|| planes::Set::<u64, 3>::try_seed(CAPACITY, ()));
    let defaults = defaults.expect("the default plane set must allocate");
    assert_eq!(alloc, [1, CAPACITY * ENTRY_BYTES, 0, 0]);
    let ((), alloc) = Tracker::measure(move || assert_eq!(defaults.len(), CAPACITY));
    assert_eq!(alloc, [0, 0, 1, CAPACITY * ENTRY_BYTES]);

    let (result, alloc) = Tracker::measure(|| planes::Set::<u64, 3>::try_seed(CAPACITY, Ordinal));
    let mut planes = result.expect("the 257-entry plane set must allocate");
    assert_eq!(alloc, [1, CAPACITY * ENTRY_BYTES, 0, 0]);

    let ((), alloc) = Tracker::measure(|| {
        for index in 0..CAPACITY {
            let entry = planes.entry(index);
            let mut entry = entry.expect("allocated entry must exist");
            entry.plane::<0>().link_back();
            if index & 1 == 0 {
                entry.plane::<1>().link_back();
            }
            if index % 3 == 0 {
                entry.plane::<2>().link_back();
            }
        }
        for index in (0..CAPACITY).step_by(2) {
            let entry = planes.entry(index);
            let mut entry = entry.expect("allocated entry must exist");
            entry.plane::<0>().unlink();
        }
        planes.clear::<1>();
        planes.clear::<2>();
    });
    assert_eq!(alloc, [0, 0, 0, 0]);

    let ((), alloc) = Tracker::measure(move || {
        assert_eq!(planes.len(), CAPACITY);
        assert_ne!(planes.len_in::<0>(), 0);
        assert_eq!(planes.len_in::<1>(), 0);
        assert_eq!(planes.len_in::<2>(), 0);
    });
    assert_eq!(alloc, [0, 0, 1, CAPACITY * ENTRY_BYTES]);
}

#[test]
fn independent_fifo_order() {
    let seed = Sequence::try_new(10).expect("factor");
    let result = planes::Set::<usize, 3>::try_seed(6, seed);
    let mut planes = result.expect("the six-entry plane set must allocate");

    let entry = planes.entry(1);
    let mut entry = entry.expect("entry one must exist");
    entry.plane::<0>().link_back();
    let entry = planes.entry(3);
    let mut entry = entry.expect("entry three must exist");
    entry.plane::<0>().link_back();
    entry.plane::<1>().link_back();
    entry.plane::<2>().link_back();
    let entry = planes.entry(2);
    let mut entry = entry.expect("entry two must exist");
    entry.plane::<1>().link_back();
    let entry = planes.entry(5);
    let mut entry = entry.expect("entry five must exist");
    entry.plane::<2>().link_back();

    let entry = planes.entry(3);
    let entry = entry.expect("entry three must exist");
    assert!(entry.is_linked::<0>());
    assert!(entry.is_linked::<1>());
    assert!(entry.is_linked::<2>());

    assert!(planes.iter::<0>().eq([(1, &10), (3, &30)]));
    assert!(planes.iter::<1>().eq([(3, &30), (2, &20)]));
    assert!(planes.iter::<2>().eq([(3, &30), (5, &50)]));

    let front = planes.front_entry::<0>();
    let front = front.expect("plane zero must have a front");
    assert_eq!(front.pop().index(), 1);
    let front = planes.front_entry::<0>();
    let front = front.expect("plane zero must have a front");
    assert_eq!(front.index(), 3);
    let front = planes.front_entry::<1>();
    let front = front.expect("plane one must have a front");
    assert_eq!(front.pop().index(), 3);
    let front = planes.front_entry::<2>();
    let front = front.expect("plane two must have a front");
    assert_eq!(front.pop().index(), 3);
    let entry = planes.entry(3).expect("shared payload remains resident");
    assert!(entry.is_linked::<0>());
    assert!(!entry.is_linked::<1>());
    assert!(!entry.is_linked::<2>());
    assert!(
        planes
            .entry(2)
            .expect("plane one survivor")
            .is_linked::<1>()
    );
    assert!(
        planes
            .entry(5)
            .expect("plane two survivor")
            .is_linked::<2>()
    );
    assert!(planes.iter::<0>().eq([(3, &30)]));
    assert!(planes.iter::<1>().eq([(2, &20)]));
    assert!(planes.iter::<2>().eq([(5, &50)]));
    let front = planes.front_entry::<2>();
    let front = front.expect("plane two must have a front");
    assert_eq!(front.pop().index(), 5);
    assert!(planes.iter::<2>().next().is_none());
    assert_eq!(planes.get(3), Some(&30));
}

#[test]
fn entry_transitions() {
    let seed = Sequence::try_new(1).expect("factor");
    let result = planes::Set::<usize, 3>::try_seed(4, seed);
    let mut planes = result.expect("the four-entry transition set must allocate");
    {
        let entry = planes.entry(2);
        let mut entry = entry.expect("entry two must exist");
        *entry = 22;
        entry.plane::<0>().link_back();
        entry.plane::<1>().link_back();
    }

    let ((), alloc) = Tracker::measure(|| {
        let front = planes.front_entry::<0>();
        let front = front.expect("plane zero must contain entry two");
        let mut entry = front.pop();
        assert_eq!(*entry, 22);
        assert!(entry.is_linked::<1>());
        entry.plane::<1>().unlink();
        *entry = 23;
    });
    assert_eq!(alloc, [0, 0, 0, 0]);
    assert_eq!(planes.get(2), Some(&23));
    assert_eq!(planes.len_in::<0>(), 0);
    assert_eq!(planes.len_in::<1>(), 0);
}

#[test]
fn rotate_front() {
    let seed = Sequence::try_new(1).expect("factor");
    let result = planes::Set::<usize, 3>::try_seed(4, seed);
    let mut planes = result.expect("required fixture outcome");
    for index in [1, 3, 2] {
        let entry = planes.entry(index);
        let mut entry = entry.expect("assertion failed: entry.is_some()");
        entry.plane::<0>().link_back();
        entry.plane::<1>().link_back();
    }
    let ((), alloc) = Tracker::measure(|| {
        for expected in [1, 3, 2, 1] {
            let front = planes.front_entry::<0>();
            let mut front = front.expect("assertion failed: front.is_some()");
            assert_eq!(front.index(), expected);
            *front += 10;
            front.move_to_back();
            assert_eq!(planes.len_in::<0>(), 3);
            assert_eq!(planes.len_in::<1>(), 3);
            assert_eq!(planes.len_in::<2>(), 0);
        }
        for expected in [3, 2] {
            let front = planes.front_entry::<0>();
            let front = front.expect("assertion failed: front.is_some()");
            assert_eq!(front.pop().index(), expected);
        }
        let front = planes.front_entry::<0>();
        let front = front.expect("assertion failed: front.is_some()");
        front.move_to_back();
        assert_eq!(planes.len_in::<0>(), 1);
        assert_eq!(planes.iter::<0>().next(), Some((1, &21)));
        for expected in [1, 3, 2] {
            let front = planes.front_entry::<1>();
            let front = front.expect("assertion failed: front.is_some()");
            assert_eq!(front.pop().index(), expected);
        }
    });
    assert_eq!(alloc, [0, 0, 0, 0]);
    assert_eq!(planes.len_in::<1>(), 0);
    assert_eq!(planes.get(1), Some(&21));
    assert_eq!(planes.get(3), Some(&13));
    assert_eq!(planes.get(2), Some(&12));
}

#[test]
fn growth_preserves_state() {
    let seed = Sequence::try_new(1).expect("factor");
    let result = planes::Set::<usize, 3>::try_seed(4, seed);
    let mut planes = result.expect("the four-entry growth set must allocate");
    assert_eq!(planes.resolve(4), None);
    assert_eq!(planes.resolve(usize::MAX), None);
    let resolved = planes.resolve(1);
    let (position, value) = resolved.expect("entry one must resolve");
    assert_eq!(position.get(), 2);
    assert_eq!(*value, 1);
    let entry = planes.entry(1);
    let mut entry = entry.expect("entry one must exist");
    entry.plane::<0>().link_back();
    let entry = planes.entry(3);
    let mut entry = entry.expect("entry three must exist");
    entry.plane::<0>().link_back();
    entry.plane::<1>().link_back();

    let result = planes.try_grow_to(8, Sequence::try_new(1).expect("factor"));
    let grown = result.expect("growth from four to eight entries must allocate");
    assert_eq!(grown, 4..8);
    assert_eq!(planes.len(), 8);
    let front = planes.front_entry::<0>();
    let front = front.expect("plane zero must have a front");
    assert_eq!(front.position(), position);
    let entry = front.pop();
    assert_eq!(entry.position(), position);
    assert_eq!(entry.index(), 1);
    let front = planes.front_entry::<0>();
    let front = front.expect("plane zero must have a front");
    assert_eq!(front.pop().index(), 3);
    let entry = planes.entry(3);
    let entry = entry.expect("entry three must remain resident");
    assert!(entry.is_linked::<1>());
    let entry = planes.entry(7);
    let mut entry = entry.expect("grown entry seven must exist");
    entry.plane::<2>().link_back();
    assert_eq!(planes.get(7), Some(&7));
    assert_eq!(planes.resolve(8), None);
}

#[test]
fn borrowed_lifetime() {
    let owner = String::from("resident");
    let result = planes::Set::<&str, 3>::try_seed(1, ());
    let mut planes = result.expect("the borrowed one-entry set must allocate");
    let entry = planes.entry(0);
    let mut entry = entry.expect("entry zero must exist");
    *entry = owner.as_str();
    entry.plane::<2>().link_back();
    assert_eq!(planes.get(0), Some(&"resident"));
    let value = planes.entry(0);
    let mut value = value.expect("entry zero must exist");
    *value = &owner[1..];
    let front = planes.front_entry::<2>();
    let front = front.expect("plane two must have a front");
    assert_eq!(front.pop().index(), 0);
    assert_eq!(planes.get(0), Some(&"esident"));
}

#[test]
fn topology_keeps_payloads() {
    let drops = Rc::new(cell::Cell::new(Some(0)));
    {
        let result = planes::Set::<DropValue, 3>::try_seed(3, Drops(Rc::clone(&drops)));
        let mut planes = result.expect("the three-payload topology must allocate");

        let entry = planes.entry(0);
        let mut entry = entry.expect("entry zero must exist");
        assert_eq!(entry.index.get(), 0);
        entry.plane::<0>().link_back();
        entry.plane::<1>().link_back();
        entry.plane::<2>().link_back();
        entry.plane::<1>().unlink();
        let front = planes.front_entry::<0>();
        let front = front.expect("plane zero must have a front");
        assert_eq!(front.pop().index(), 0);
        planes.clear::<2>();
        assert_eq!(drops.get(), Some(0));

        let result = planes.try_grow_to(5, Drops(Rc::clone(&drops)));
        let grown = result.expect("growth from three to five payloads must allocate");
        assert_eq!(grown, 3..5);
        assert_eq!(planes.len(), 5);
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(5));
}

#[test]
fn installed_payload() {
    use o3::depot::map;
    let drops = cell::Cell::new(Some(0));
    {
        let mut slots = Table::<[u64; 1], Block<'_>, 3>::try_new(map::Plan::fixed::<2>(), 2)
            .expect("payload storage");
        let entry = slots.try_vacant(1, 7, [7]).expect("allocated slot");
        let mut entry = entry.insert(Block {
            bytes: [7; 4096],
            drops: Counted(&drops),
        });
        let address = entry.bytes.as_ptr();
        let ((), allocs) = Tracker::measure(|| {
            let (value, state) = entry.split::<1>();
            let State::Vacant(vacant) = state else {
                panic!("the newly installed payload starts detached");
            };
            let linked = vacant.link_back();
            value.bytes[0] = 19;
            linked.unlink().link_back();
            entry.plane::<2>().link_back();
            assert!(matches!(entry.plane::<1>(), State::Linked(_)));
            assert!(matches!(entry.plane::<2>(), State::Linked(_)));
            assert_eq!(entry.bytes.as_ptr(), address);
            entry.plane::<1>().unlink();
            entry.plane::<2>().unlink();
            entry.plane::<1>().link_back();
            assert_eq!(entry.bytes[0], 19);
            {
                let (key, value) = entry.remove();
                assert_eq!(key, [7]);
                assert_eq!(value.bytes[0], 19);
                assert_eq!(drops.get(), Some(0));
            }
            assert_eq!(drops.get(), Some(1));
        });
        assert_eq!(allocs, [0, 0, 0, 0]);
        assert!(slots.is_empty::<0>());
        assert!(slots.is_empty::<1>());
        assert!(slots.is_empty::<2>());
        assert!(slots.at(1).is_none());
    }
    assert_eq!(drops.get(), Some(1));
}

#[test]
fn scoped_resident_reuses() {
    use std::ptr;

    use common::Owner;
    use o3::{
        depot::{capacity::Limit, heap::indexed},
        mem::slot::Storage,
    };

    let drops = cell::Cell::new(Some(0));
    let mut set: planes::Set<
        Storage<Owner<'_>, usize>,
        3,
        groups::Index<u64, Limit<4>, u64, 2, 4>,
    > = planes::Set::<Storage<Owner<'_>, usize>, 3, indexed::Min<u64, Limit<4>>>::try_seed_in::<0>(
        Limit::fixed::<3>(),
        Ordinal,
    )
    .expect("physical rows")
    .try_into()
    .expect("reset rows");
    let records = groups::Index::<u64, Limit<4>, u64, 2, 4>::reserve(Limit::fixed::<1>())
        .expect("smaller active prefix");
    set.try_replace::<0>(records, Ordinal)
        .expect("retained physical rows")();
    assert_eq!((set.len(), set.limit().get(), set.len_in::<0>()), (3, 1, 1));
    let ((), activity) = Tracker::measure(|| {
        for (value, count) in [(7, 1), (9, 2)] {
            bound::Scope::with(&mut set, |root| {
                let (mut entries, mut side) = root.split();
                assert!(entries.resolve(1).is_none());
                let (key, front) = entries.front_entry::<0>().expect("free active row");
                let at = key.position();
                front.pop();
                let (mut resident, key) = entries.install::<0>(
                    key,
                    Owner {
                        drops: Counted(&drops),
                        value,
                    },
                );
                let pointer = ptr::from_ref(resident.get(key));
                resident.get_mut(key).value += 1;
                let (timer, mut entry) = resident.select(key);
                drop(timer);
                assert_eq!(ptr::from_ref(entry.get()), pointer);
                entry.link_back::<1>();
                assert!(entry.is_linked::<1>());
                entry.unlink::<1>();
                assert_eq!(key.active().position(), at);
                assert_eq!(*resident.metadata(key), 0);
                assert!(side.bucket(0).next().is_none());
                side.group(key.active())
                    .clear()
                    .insert([Some(value as u64), None], |value| (0_u64, value));
                let (found, item) = side.bucket(0).next().expect("active reset row");
                assert_eq!((found.index(), *item), (0, value as u64));
                let (timer, terminal) = resident.into_select(key, side);
                drop(timer);
                let (item, mut entry) = terminal.take();
                assert_eq!(item.value, value + 1);
                assert!(entry.get().is_none());
                assert_eq!(*entry.metadata(), 0);
                entry.plane::<0>().link_back();
                drop(item);
            });
            assert_eq!(drops.get(), Some(count));
        }
    });
    assert_eq!(activity, [0; 4]);
    drop(set);
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn abandoned_resize_keeps() {
    use o3::{
        depot::{capacity::Limit, heap::indexed},
        mem::slot::Storage,
    };

    for (old, next) in [(3, 1), (1, 3)] {
        let old = Limit::<4>::new(old).expect("old prefix");
        let next = Limit::<4>::new(next).expect("new prefix");
        let mut set: planes::Set<_, 3, groups::Index<u64, Limit<4>, u64, 2, 4>> =
            planes::Set::<Storage<u8, usize>, 3, indexed::Min<u64, Limit<4>>>::try_seed_in::<0>(
                old, Ordinal,
            )
            .expect("physical rows")
            .try_into()
            .expect("reset rows");
        let records =
            groups::Index::<u64, Limit<4>, u64, 2, 4>::reserve(next).expect("replacement rows");
        drop(
            set.try_replace::<0>(records, Ordinal)
                .expect("staged resize"),
        );
        assert_eq!(
            (set.len(), set.limit(), set.len_in::<0>()),
            (old.get().max(next.get()), old, old.get())
        );
        assert!(set.iter::<0>().map(|(index, _)| index).eq(0..old.get()));
        bound::Scope::with(&mut set, |root| {
            let (entries, _) = root.split();
            assert!(entries.resolve(old.get() - 1).is_some());
            assert!(entries.resolve(old.get()).is_none());
        });
    }
}
