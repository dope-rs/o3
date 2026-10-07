pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{marker, mem};

use common::{
    alloc::system::Tracker,
    depot::arena::slab::{Aligned, Sent, Slot, Tag},
};
use o3::depot::{
    self,
    arena::slab::{self, entries, key},
    capacity,
    index::Access as _,
    queue::links,
};

#[test]
fn identity_follows_insert() {
    let result = slab::Store::<u8, Tag, 3>::try_new(capacity::Count::new(1));
    let mut table = result.expect("the one-slot exclusive table must allocate");
    {
        let vacant = table.try_vacant();
        assert!(vacant.is_ok());
    }
    let first = (table.insert(1)).expect("vacant exclusive slot");
    assert_eq!(first.generation().get(), 1);
    assert_eq!(table.remove(first), Some(1));

    let vacant = (table.try_vacant()).expect("the removed slot must be reusable");
    let epoch =
        (first.generation().checked_add(1)).expect("the discarded staging generation must exist");
    let stale = key::Handle::from_parts(key::Parts::new(first.index(), epoch));
    {
        let staged = vacant.stage_with(|index| {
            assert_eq!(index.raw(), stale.index());
            0
        });
        assert_eq!(*staged.get(), 0);
    }
    assert_eq!(stale.generation().get(), 2);
    let live = (table.insert(2)).expect("vacant slot after discarded staging");
    assert_eq!(stale.index(), live.index());
    assert_eq!(live.generation().get(), 3);
    assert_eq!(table.try_get(stale).ok(), None);
    assert_eq!(table.try_get(live).ok(), Some(&2));
}

#[test]
fn owner_remains_unpin() {
    fn assert_unpin<T: Unpin>() {}

    assert_unpin::<slab::Store<marker::PhantomPinned>>();
    let result = slab::Store::<marker::PhantomPinned>::try_new(capacity::Count::new(1));
    let mut table = result.expect("the one-slot Unpin table must allocate");
    let key = (table.insert(marker::PhantomPinned)).expect("vacant pinned-value slot");
    let moved = table;
    assert!(moved.try_get(key).ok().is_some());
}

#[test]
fn keeps_allocation_layout() {
    assert_eq!(mem::size_of::<slab::Store<u64>>(), 32);
    assert_eq!(mem::size_of::<Option<slab::Store<u64>>>(), 32);
    assert_eq!(
        mem::size_of::<slab::Vacant<entries::Pending<'static, u64>>>(),
        16
    );
    assert_eq!(
        mem::size_of::<Option<slab::Vacant<entries::Pending<'static, u64>>>>(),
        16
    );
    assert_eq!(
        mem::size_of::<slab::Vacant<entries::Pending<'static, u64, { u32::MAX }, entries::Interior>>>(
        ),
        16
    );
    assert_eq!(
        mem::size_of::<
            Option<slab::Vacant<entries::Pending<'static, u64, { u32::MAX }, entries::Interior>>>,
        >(),
        16
    );
    assert_eq!(
        mem::size_of::<
            slab::Staged<o3::mem::Lease<entries::Busy<'static, u64>, o3::mem::lease::Open>>,
        >(),
        16
    );
    assert_eq!(mem::size_of::<slab::Entry<'static, u64>>(), 24);
    assert_eq!(mem::size_of::<slab::Reuse<'static, u64>>(), 24);
    assert_eq!(mem::size_of::<Option<slab::Reuse<'static, u64>>>(), 24);
    for count in [0, 1, 3, 8] {
        let bytes = (count as usize * 28 + 7) & !7;
        let calls = usize::from(count != 0);
        let (built, cold) =
            Tracker::measure(|| slab::Store::<u64>::try_new(capacity::Count::new(count)));
        assert_eq!(cold, [calls, bytes, 0, 0]);
        let freed = {
            let mut table = (built).expect("the measured slab extent must allocate");
            let ((), hot) = Tracker::measure(|| {
                for value in 0..count {
                    table
                        .insert(u64::from(value))
                        .as_ref()
                        .expect("required fixture outcome");
                }
                for entry in slab::Slots::new(&mut table).entries() {
                    let mut entry =
                        into_occupied(entry).expect("every measured slot was initialized");
                    *entry.get_mut() += 1;
                }
                assert!(
                    (slab::Slots::new(&mut table)
                        .entries()
                        .map(|entry| {
                            let entry = into_occupied(entry).expect("initialized resident");
                            *entry.get()
                        })
                        .sum::<u64>())
                        == (u64::from(count) * u64::from(count + 1) / 2)
                );
            });
            assert_eq!(hot, [0; 4]);
            Tracker::begin()
        };
        let freed = freed.finish();
        assert_eq!((freed.allocations, freed.allocated_bytes), (0, 0));
        assert_eq!(
            (freed.deallocations, freed.deallocated_bytes),
            (calls, bytes)
        );
    }
}

#[test]
fn aligns_both_planes() {
    let (built, cold) =
        Tracker::measure(|| slab::Store::<Aligned>::try_new(capacity::Count::new(3)));
    assert_eq!(cold, [1, 448, 0, 0]);
    let freed = {
        let mut table = (built).expect("the over-aligned slab must allocate");
        for value in 0..3 {
            match table.insert(Aligned(value)) {
                Ok(_) => {}
                outcome => {
                    assert!(outcome.is_ok(), "required fixture outcome");
                    return;
                }
            }
        }
        for (index, entry) in slab::Slots::new(&mut table).entries().enumerate() {
            let mut entry = into_occupied(entry).expect("each aligned slot was initialized");
            let value = entry.get_mut();
            assert_eq!(
                value as *mut Aligned as usize % mem::align_of::<Aligned>(),
                0
            );
            assert_eq!(value.0, index as u8);
            value.0 += 1;
        }
        assert!(
            (slab::Slots::new(&mut table)
                .entries()
                .map(|entry| {
                    let entry = into_occupied(entry).expect("aligned resident");
                    u32::from(entry.get().0)
                })
                .sum::<u32>())
                == (6)
        );
        Tracker::begin()
    };
    let freed = freed.finish();
    assert_eq!((freed.allocations, freed.allocated_bytes), (0, 0));
    assert_eq!((freed.deallocations, freed.deallocated_bytes), (1, 448));
}

#[test]
fn keeps_disjoint_slots() {
    let mut table = (slab::Store::<u64>::try_new(capacity::Count::new(4)))
        .expect("allocate four physical slots");
    let old = (table.insert(11)).expect("insert the first physical slot");
    let (live, next, hot) = {
        let (reused, hot) = Tracker::measure(|| {
            let mut entries = slab::Slots::new(&mut table).entries();
            let occupied =
                into_occupied((entries.next()).expect("the first slot must retain its value"))
                    .expect("the first slot must retain its value");
            let mut free = entries.map(|entry| {
                let (entry,) = match entry {
                    slab::Entry::Vacant(entry) => Some((entry,)),
                    _ => None,
                }
                .expect("remaining slots must be vacant");
                entry
            });
            let a = free.next();
            assert!(a.is_some(), "first reservation");
            let c = {
                let b = free.next();
                assert!(b.is_some(), "second reservation");
                let c = free.next().expect("third reservation");
                assert!(free.next().is_none());
                c
            };
            slab::Reuse::try_from(occupied).map(|reuse| {
                let (live, prior) = reuse.replace(22);
                assert_eq!(prior, 11);
                assert_eq!(*live.get(), 22);
                let next = c.insert(33);
                assert!((live.key()) != (next));
                (live.key(), next)
            })
        });
        match reused {
            Ok((live, next)) => (live, next, hot),
            outcome => {
                assert!(outcome.is_ok(), "occupied generation must be reusable");
                return;
            }
        }
    };
    assert_eq!(hot, [0; 4]);
    assert_eq!(table.try_get(old).ok(), None);
    assert_eq!(table.try_get(live).ok(), Some(&22));
    assert_eq!(table.try_get(next).ok(), Some(&33));
    assert!(!table.is_full());
    table.insert(44).as_ref().expect("required fixture outcome");
    table.insert(55).as_ref().expect("required fixture outcome");
    assert!((table.insert(66)) == (Err(66)));
}

#[test]
fn reuse_keeps_generation() {
    let mut table = (slab::Store::<u64, (), 2>::try_new(capacity::Count::new(1)))
        .expect("allocate one physical slot");
    let old = (table.insert(11)).expect("insert the initial generation");
    {
        let entry =
            (table.try_entry(old)).expect("slab key must retain its original slot observation");

        let reuse = slab::Reuse::try_from(entry);
        match &reuse {
            Ok(_) => {}
            outcome => {
                assert!(outcome.is_ok(), "required fixture outcome");
                return;
            }
        }
    }
    assert_eq!(table.try_get(old).ok(), Some(&11));
    let entry = (table.try_entry(old)).expect("slab key must retain its original slot observation");

    match slab::Reuse::try_from(entry) {
        Ok(reuse) => {
            let (last, prior) = reuse.replace(22);
            assert_eq!(prior, 11);
            assert_eq!(last.key().index(), old.index());
            assert_eq!(last.key().generation().get(), 2);
            let entry = {
                let result = slab::Reuse::try_from(last);
                assert!(result.is_err(), "the final generation cannot be reused");
                match result {
                    Err(error) => error,
                    Ok(_) => return,
                }
            };
            assert_eq!(*entry.get(), 22);
            assert_eq!(entry.remove(), 22);
        }
        outcome => {
            assert!(outcome.is_ok(), "remaining generation must be reusable");
            return;
        }
    }
    assert!(table.is_full());
    assert!((table.insert(33)) == (Err(33)));
}

#[test]
fn remove_sees_busy_slot() {
    let slab = (slab::Cell::<u32, (), { u32::MAX }, false>::try_new(capacity::Count::new(1)))
        .expect("the single-slot remove reentry slab must allocate");
    let key =
        (slab.vacancy().map(|vacant| vacant.insert(42))).expect("empty slab accepts one value");
    let value = slab.update(key, |value| {
        assert_eq!(
            slab.entry(key.parts()).map(|entry| {
                let value = *entry.value();
                entry.discard();
                value
            }),
            None
        );
        *value
    });
    assert_eq!(value, Some(42));
    assert_eq!(
        slab.entry(key.parts()).map(|entry| {
            let value = *entry.value();
            entry.discard();
            value
        }),
        Some(42)
    );
}

#[test]
fn update_keeps_slot() {
    let slab = (slab::Cell::<u32, (), { u32::MAX }, false>::try_new(capacity::Count::new(1)))
        .expect("the single-slot update reentry slab must allocate");
    let key =
        (slab.vacancy().map(|vacant| vacant.insert(42))).expect("empty slab accepts one value");
    let first = slab.update(key, |value| value as *mut u32);
    assert!(first.is_some(), "inserted key remains occupied");
    let second = slab.update(key, |value| value as *mut u32);
    assert_eq!(first, second);
}

#[test]
fn keep_restores_slot() {
    let slab = (slab::Cell::<u32, (), { u32::MAX }, false>::try_new(capacity::Count::new(1)))
        .expect("the single-slot keep reentry slab must allocate");
    let key =
        (slab.vacancy().map(|vacant| vacant.insert(5))).expect("empty slab accepts one value");
    {
        let mut entry = slab::Slots::new(&slab)
            .entry_at(key.index())
            .expect("inserted slot");
        *entry.value_mut() = 999;
    }
    assert!((slab.len()) == (capacity::Count::new(1)));
    assert_eq!(
        slab::Slots::new(&slab)
            .entry_at(key.index())
            .map(|entry| entry.remove()),
        Some(999)
    );
    assert_eq!(slab.len(), capacity::Count::EMPTY);
}

// SAFETY: the fixture retains its just-inserted slot's exclusive owner.

#[test]
fn retains_owner_and_plane() {
    let mut table =
        (slab::Store::<u64>::try_new(capacity::Count::new(1))).expect("one slot must allocate");
    let key = (table.insert(17)).expect("the vacant slot must accept its value");
    let mut context = 29;
    let ((value, observed), delta) = Tracker::measure(|| {
        let live = unsafe { Slot::new(&mut table, key) }.into_live();
        // SAFETY: this shared view retains the original slab and the same initialized slot and plane.
        let read = unsafe {
            live.as_ref().map(|(slot, key)| {
                (
                    slot.owner(),
                    <links::Index as depot::raw::Word>::from_offset(key.index() as usize),
                )
            })
        };
        let read = slab::Store::<_>::value(read);
        assert_eq!(**read.source(), 17);
        // SAFETY: this splits the retained root without changing its slot or plane.
        let live = unsafe {
            live.map(|(slot, key)| {
                (
                    (slot.into_owner(), &mut context),
                    <links::Index as depot::raw::Word>::from_offset(key.index() as usize),
                )
            })
        };
        let (mut slot, context): (
            slab::Occupied<entries::Busy<'_, u64, { u32::MAX }, entries::Exclusive, 1, Sent>>,
            _,
        ) = slab::Occupied::from_live(live);
        assert_eq!(*slot.get(), 17);
        *slot.get_mut() = 43;
        *context += 1;
        let observed = slot.key();
        let (vacant, value) = slot.take();
        let slot: slab::Occupied<
            entries::Busy<'_, u64, { u32::MAX }, entries::Exclusive, 1, Sent>,
        > = vacant.insert(value);
        (slot.remove(), observed)
    });
    assert_eq!((value, context), (43, 30));
    assert!((observed) == (key));
    assert_eq!(delta, [0, 0, 0, 0]);
    assert_eq!(table.len(), o3::depot::capacity::Count::EMPTY);
    assert_eq!(table.try_get(key).ok(), None);
    let reused = (table.insert(71)).expect("removal must release the slot");
    assert_eq!(reused.index(), key.index());
    assert!((reused.generation()) != (key.generation()));
    assert_eq!(table.try_get(reused).ok(), Some(&71));
}

fn into_occupied<T>(
    entry: slab::Entry<'_, T>,
) -> Option<slab::Occupied<entries::Busy<'_, T>, (), false>> {
    match entry {
        slab::Entry::Occupied(entry) => Some(entry),
        _ => None,
    }
}
