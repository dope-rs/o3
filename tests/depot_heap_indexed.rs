pub mod common;

use std::{alloc::Layout, mem};

use common::alloc::system::Tracker;
use o3::{
    alloc::Overflow,
    depot::{
        capacity::Count,
        heap::{self, indexed},
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn direct_position_width() {
    const CAPACITY: usize = 257;
    const BYTES: usize = CAPACITY * 3 * mem::size_of::<u32>();
    let (heap, allocs) =
        Tracker::measure(|| indexed::Min::<u32>::try_new(Count::new(CAPACITY as u32)));
    let mut heap = heap.expect("the heap owner must be available");

    assert_eq!(allocs, [1, BYTES, 0, 0]);

    let ((), allocs) = Tracker::measure(|| {
        for index in 0..CAPACITY {
            let entry = heap.try_entry(index);
            let entry = match entry {
                Ok(indexed::Entry::Vacant(entry)) => Some(entry),
                _ => None,
            }
            .expect("each direct index must be inserted once");
            entry.set(index as u32);
        }
        while heap
            .first_entry()
            .map(|entry| (entry.index(), entry.take()))
            .is_some()
        {}
    });
    assert_eq!(allocs, [0; 4]);

    let (heap, allocs) =
        Tracker::measure(|| indexed::Min::<u32>::try_repeat(Count::new(CAPACITY as u32), 7));
    let heap = heap.expect("the heap owner must be available");

    assert_eq!(allocs, [1, BYTES, 0, 0]);
    let ((), allocs) = Tracker::measure(move || assert_eq!(heap.len(), CAPACITY));
    assert_eq!(allocs, [0, 0, 1, BYTES]);
}

#[test]
fn layout_failure_is_exact() {
    type Wide = [u8; isize::MAX as usize / u32::MAX as usize + 1];

    let (result, activity) = Tracker::measure(|| indexed::Min::<Wide>::try_new(Count::MAX));
    let (capacity, plane, entries, offsets, values, error) =
        into_layout((result).err().expect("the allocation must fail"))
            .expect("the oversized entry plane must fail before allocation");
    assert!((capacity) == (Count::MAX));
    assert!((plane) == (heap::Plane::Entries));
    assert!(entries.size() >= mem::size_of::<Wide>());
    assert_eq!(offsets, Layout::new::<u32>());
    assert_eq!(values, Layout::new::<()>());
    assert_eq!(error, o3::alloc::Overflow);
    assert_eq!(activity, [0; 4]);
}

#[test]
fn positions_overflow() {
    const WIDTH: usize =
        (isize::MAX as usize / u32::MAX as usize).saturating_sub(mem::size_of::<u32>());
    type Wide = [u8; WIDTH];

    let bytes = (WIDTH + mem::size_of::<u32>()).next_multiple_of(mem::align_of::<u32>());
    let count = Count::new((isize::MAX as usize / bytes).min(u32::MAX as usize) as u32);
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || indexed::Min::<Wide>::try_new(count)));
    let (capacity, plane, entries, offsets, values, error) =
        into_layout((result).err().expect("the allocation must fail"))
            .expect("positions must overflow before any allocation attempt");
    assert!((capacity) == (count));
    assert!((plane) == (heap::Plane::Positions));
    assert!(
        entries
            .size()
            .checked_mul(count.get())
            .is_some_and(|size| size <= isize::MAX as usize)
    );
    assert_eq!(offsets, Layout::new::<u32>());
    assert_eq!(values, Layout::new::<()>());
    assert_eq!(error, o3::alloc::Overflow);
    assert_eq!(activity, [0; 4]);
}

#[test]
fn bounded_repeat_domain() {
    use o3::depot::capacity::Limit;

    assert_eq!(
        mem::size_of::<indexed::Min<u32, Limit<65536>>>(),
        3 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<indexed::Min<u32, Limit<65536>>>>(),
        3 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<heap::Error<Limit<65536>>>(),
        mem::size_of::<heap::Error>()
    );

    let capacity = Limit::<65536>::fixed::<7>();
    let (heap, allocs) = Tracker::measure(|| indexed::Min::try_repeat(capacity, 11u32));
    let mut heap = heap.expect("the heap owner must be available");

    assert_eq!(allocs, [1, 84, 0, 0]);
    assert_eq!(heap.len(), 7);
    let ((), allocs) = Tracker::measure(|| {
        for index in 0..7 {
            assert_eq!(heap.get(index), Some(&11));
            let entry = heap.try_entry(index);
            let entry = match entry {
                Ok(indexed::Entry::Occupied(entry)) => Some(entry),
                _ => None,
            }
            .expect("every repeated slot must be occupied");
            entry.set(index as u32);
        }
        for index in 0..7 {
            assert_eq!(
                heap.first_entry()
                    .map(|entry| (entry.index(), entry.take())),
                Some((index, index as u32))
            );
        }
    });
    assert_eq!(allocs, [0; 4]);
    let ((), allocs) = Tracker::measure(move || {
        assert!(heap.is_empty());
    });
    assert_eq!(allocs, [0, 0, 1, 84]);
}

#[test]
fn empty_owner_and_niche() {
    use std::mem;

    assert_eq!(mem::size_of::<indexed::Min<u64>>(), 24);
    assert_eq!(mem::size_of::<Option<indexed::Min<u64>>>(), 24);
    let (result, activity) = Tracker::measure(|| indexed::Min::<()>::try_new(Count::EMPTY));
    let heap = result.expect("the heap owner must be available");

    assert_eq!(activity, [0; 4]);
    let ((), activity) = Tracker::measure(move || {
        assert!(heap.is_empty());
    });
    assert_eq!(activity, [0; 4]);
}

#[test]
fn aligned_planes() {
    use common::depot::heap::Aligned;

    let (result, activity) =
        Tracker::measure(|| indexed::Min::try_repeat(Count::new(3), Aligned(7)));
    let heap = result.expect("the heap owner must be available");

    assert_eq!(activity, [1, 448, 0, 0]);
    let ((), activity) = Tracker::measure(move || {
        for index in 0..3 {
            let value = heap
                .get(index)
                .expect("repeat must initialize every aligned entry");
            assert!((*value) == (Aligned(7)));
            assert_eq!(std::ptr::from_ref(value).addr() % 64, 0);
        }
    });
    assert_eq!(activity, [0, 0, 1, 448]);
}

#[test]
fn retains_checked_extent() {
    use o3::depot::capacity::Limit;

    let capacity = Limit::<65536>::fixed::<3>();
    let heap = indexed::Min::<u64, _>::try_new(capacity);
    let mut heap = heap.expect("the heap owner must be available");

    for index in [3, usize::MAX] {
        let error = heap
            .try_entry(index)
            .err()
            .expect("an index outside the exact extent must fail");
        assert_eq!(error.index(), index);
        assert!((error.capacity()) == (capacity));
    }
    assert!(heap.is_empty());
    let entry = heap.try_entry(2);
    let entry = match entry {
        Ok(indexed::Entry::Vacant(entry)) => Some(entry),
        _ => None,
    }
    .expect("rejected indices must leave the final valid slot vacant");
    let entry = entry.insert(17);
    assert_eq!((entry.index(), *entry.priority()), (2, 17));
}

#[test]
fn retains_moving_position() {
    let heap = indexed::Min::<u32>::try_repeat(Count::new(7), 20);
    let mut heap = heap.expect("the heap owner must be available");

    let ((), activity) = Tracker::measure(|| {
        let entry = heap.try_entry(6);
        let entry = match entry {
            Ok(indexed::Entry::Occupied(entry)) => Some(entry),
            _ => None,
        }
        .expect("the repeated final slot must be occupied");
        let (entry, old) = entry.replace(0);
        assert_eq!(old, 20);
        assert_eq!((entry.index(), *entry.priority()), (6, 0));
        let (entry, old) = entry.replace(40);
        assert_eq!(old, 0);
        assert_eq!((entry.index(), *entry.priority()), (6, 40));
        let (vacant, old) = entry.remove();
        assert_eq!(old, 40);
        let entry = vacant.insert(1);
        assert_eq!((entry.index(), *entry.priority()), (6, 1));
        let (vacant, old) = entry.remove();
        assert_eq!(old, 1);
        let entry = vacant.insert(2);
        assert_eq!((entry.index(), *entry.priority()), (6, 2));
    });
    assert_eq!(activity, [0; 4]);
    assert_eq!(heap.peek(), Some((6, &2)));
    assert_eq!(heap.len(), 7);
}

fn into_layout(
    error: heap::Error,
) -> Option<(Count, heap::Plane, Layout, Layout, Layout, Overflow)> {
    match error {
        heap::Error::Layout {
            capacity,
            plane,
            entries,
            positions: offsets,
            residents: values,
            error,
        } => Some((capacity, plane, entries, offsets, values, error)),
        _ => None,
    }
}

#[test]
fn resident_phases() {
    use std::cell::Cell;

    use common::depot::heap::resident::{Phases, payload};
    use indexed::resident::{self, Control, Entry, phase};
    use o3::depot::queue::indexed::planes;

    let drops = [const { Cell::new(Some(0)) }; 19];
    let calls = Cell::new(0);
    let failed = Tracker::fail_after(1, || {
        resident::Min::<u32, Phases<'_>>::try_repeat(Count::new(3), 10, |index| {
            calls.set(calls.get() + 1);
            (payload(index, &drops), payload(index + 3, &drops))
        })
    });
    assert!(matches!(failed, Err(planes::Error::Values { .. })));
    assert_eq!(calls.get(), 0);

    let mut owner = resident::Min::<u32, Phases<'_>>::try_repeat(Count::new(3), 10, |index| {
        (payload(index, &drops), payload(index + 3, &drops))
    })
    .expect("three resident phases");

    let ((), activity) = Tracker::measure(|| {
        let slot = owner
            .entry(0)
            .and_then(|entry| match entry {
                Entry::Scheduled(slot) => Some(slot),
                _ => None,
            })
            .expect("first resident");
        let (slot, _, _) = slot.retire(payload(6, &drops));
        slot.change::<phase::Data>(payload(7, &drops));
        owner
            .data()
            .expect("ready front")
            .change::<phase::Active>(payload(8, &drops));
        assert_eq!(owner.active(), 1);

        let slot = owner
            .entry(1)
            .and_then(|entry| match entry {
                Entry::Scheduled(slot) => Some(slot),
                _ => None,
            })
            .expect("second resident");
        let (slot, _, _) = slot.retire(payload(9, &drops));
        slot.change::<phase::Data>(payload(10, &drops));
        let slot = owner
            .entry(0)
            .and_then(|entry| match entry {
                Entry::Active(slot) => Some(slot),
                _ => None,
            })
            .expect("live resident");
        slot.schedule(0, payload(11, &drops));
        assert_eq!(owner.active(), 0);

        let slot = owner
            .entry(1)
            .and_then(|entry| match entry {
                Entry::Data(slot) => Some(slot),
                _ => None,
            })
            .expect("ready resident");
        slot.change::<phase::Control>(payload(12, &drops));
        assert!(owner.data().is_none());
        assert!(owner.peek().expect("scheduled control").3.is_none());
        let slot = owner
            .control()
            .and_then(|entry| match entry {
                Control::Full(slot) => Some(slot),
                _ => None,
            })
            .expect("oldest control");
        assert_eq!((slot.index(), slot.get().1.id), (0, 11));
        slot.change::<phase::Scheduled>(payload(13, &drops));

        let slot = owner
            .entry(0)
            .and_then(|entry| match entry {
                Entry::Scheduled(slot) => Some(slot),
                _ => None,
            })
            .expect("rescheduled resident");
        let (slot, _, _) = slot.retire(payload(14, &drops));
        slot.schedule(1, payload(15, &drops));
        owner.clear(|_, _| payload(16, &drops));
        assert!(owner.peek().is_none());
        for (index, id, next) in [(1, 12, 17), (0, 15, 18)] {
            let slot = owner
                .control()
                .and_then(|entry| match entry {
                    Control::Empty(slot) => Some(slot),
                    _ => None,
                })
                .expect("retained control order");
            assert_eq!((slot.index(), slot.get().1.id), (index, id));
            slot.change::<phase::Idle>(payload(next, &drops));
        }
        assert!(!owner.has_events());
    });
    assert_eq!(activity, [0; 4]);
    drop(owner);
    assert!(drops.iter().all(|count| count.get() == Some(1)));
}
