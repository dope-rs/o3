pub mod common;

use std::{alloc::Layout, mem};

use common::{
    alloc::system::Tracker,
    depot::{heap::resident::Aligned, index::key::Id},
};
use o3::depot::{
    self,
    arena::slab::{self, key},
    capacity::{self, Count},
    heap::{self, resident},
};

fn planes<P: Copy + Ord, R: Copy>(priority: P, resident: R) {
    for count in [0, 1, 2, 3, 7, 128, 257] {
        let entries =
            (Layout::array::<(Id<'static>, P)>(count)).expect("the fixture entry plane must fit");
        let offsets = (Layout::array::<u16>(count)).expect("the fixture position plane must fit");
        let values = (Layout::array::<R>(count)).expect("the fixture resident plane must fit");
        let (prefix, _) = (entries.extend(offsets)).expect("the fixture prefix must fit");
        let (layout, _) = (prefix.extend(values)).expect("the fixture allocation must fit");
        let bytes = layout.pad_to_align().size();
        let (result, activity) = Tracker::measure(|| {
            resident::Min::<Id<'static>, P, R, u16>::try_repeat(Count::new(count as u32), resident)
        });
        let mut heap = (result).expect("heap storage must fit its declared domain");

        assert_eq!(activity, [usize::from(bytes != 0), bytes, 0, 0]);
        let ((), activity) = Tracker::measure(move || {
            for index in 0..count {
                let (entry,) = match (heap
                    .entry(Id::try_new(index, 1).expect("identity components fit")))
                .expect("resident heap key must remain in its original domain")
                {
                    resident::Entry::Vacant(entry) => Some((entry,)),
                    _ => None,
                }
                .expect("each resident must start vacant");
                assert_eq!(
                    entry.resident() as *const R as usize % mem::align_of::<R>(),
                    0
                );
                entry.insert(priority);
                let (entry,) =
                    match heap.probe(Id::try_new(index, 1).expect("identity components fit")) {
                        heap::Probe::Occupied(entry) => Some((entry,)),
                        _ => None,
                    }
                    .expect("the inserted priority must remain live");
                let value = entry.priority();
                assert_eq!(value as *const P as usize % mem::align_of::<P>(), 0);
            }
        });
        assert_eq!(activity, [0, 0, usize::from(bytes != 0), bytes]);
    }
}

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn planes_keep_geometry() {
    planes(7_u8, Aligned(9));
    planes(Aligned(7), 9_u8);
    planes(Aligned(7), ());
    planes((), Aligned(9));
}

#[test]
fn allocation_error() {
    let count = Count::new(3);
    let (result, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            resident::Min::<Id<'static>, u64, Aligned, u16>::try_repeat(count, Aligned(7))
        })
    });
    let (capacity, layout, error) = match (result)
        .err()
        .expect("allocation rejection must retain its exact context")
    {
        heap::Error::Allocation {
            capacity,
            layout,
            error,
        } => Some((capacity, layout, error)),
        _ => None,
    }
    .expect("allocation rejection must retain its exact context");
    assert!((capacity) == (count));
    assert_eq!(layout.size(), 512);
    assert_eq!(layout.align(), 128);
    assert_eq!(error, o3::alloc::Exhausted);
    assert_eq!(activity, [0; 4]);
}

#[test]
fn retains_generations() {
    let count = capacity::Count::new(1);
    let mut source = slab::Store::<u64>::try_new(count).expect("source allocation");
    let mut heap = (resident::Min::<key::Handle, u64, u64>::try_repeat(count, 0))
        .expect("heap storage must fit its declared domain");

    let ((), allocs) = Tracker::measure(|| {
        for value in 0..4096 {
            let current = source
                .try_vacant()
                .expect("released source slot")
                .stage(value)
                .commit();
            let key = current.key();
            let size =
                mem::size_of_val(&current) + mem::size_of_val(&key) + mem::size_of::<usize>();
            // SAFETY: current is the native source in the paired one-slot allocation domain.
            let slot = unsafe { depot::raw::Proof::new(((&mut heap, current), key)) }.into();
            assert_eq!(mem::size_of_val(&slot), size);
            let (current, ()) = slot.with(|entry| {
                assert!(!matches!(&entry, resident::Entry::Occupied(_)));
                match entry {
                    resident::Entry::Vacant(mut entry) => {
                        assert_eq!(value % 8, 0);
                        assert_eq!(*entry.resident(), value);
                        *entry.resident_mut() += 1;
                        entry.insert(value);
                    }
                    resident::Entry::Conflict(mut entry) => {
                        assert!(value % 8 != 0);
                        assert_eq!(*entry.resident_mut(), value);
                        *entry.resident_mut() += 1;
                        entry.set(value);
                    }
                    resident::Entry::Occupied(_) => {}
                }
            });
            assert_eq!(*current.get(), value);
            // SAFETY: the previous operation returned the unchanged native source owner.
            let slot = unsafe { depot::raw::Proof::new(((&mut heap, current), key)) }.into();
            let (current, ()) = slot.with(|entry| {
                let entry = match entry {
                    resident::Entry::Occupied(entry) => Some(entry),
                    resident::Entry::Vacant(_) | resident::Entry::Conflict(_) => None,
                }
                .expect("same source must retain its occupied entry");
                assert_eq!(*entry.priority(), value);
                assert_eq!(*entry.resident(), value + 1);
                if value % 8 == 7 {
                    entry.retire();
                }
            });
            assert_eq!(current.remove(), value);
        }
    });
    assert_eq!(allocs, [0; 4]);
    assert!(heap.peek().is_none());
    assert_eq!(source.len(), o3::depot::capacity::Count::EMPTY);
}
