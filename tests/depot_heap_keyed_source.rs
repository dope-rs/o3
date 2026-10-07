pub mod common;

use std::{mem, num::NonZero};

use common::{alloc::system::Tracker, depot::heap::Address};
use o3::depot::{
    self,
    arena::slab::{self, key},
    capacity::{self, Count},
    heap::keyed::{self, Entry},
    queue::round,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn retains_source() {
    let count = capacity::Count::new(1);
    let mut source = slab::Store::<u64>::try_new(count).expect("source allocation");
    let heap = keyed::Min::<key::Handle, u64>::try_new(count);
    let mut heap = heap.expect("the heap owner must be available");

    let ((), allocs) = Tracker::measure(|| {
        for value in 0..4096 {
            let current = source
                .try_vacant()
                .expect("released source slot")
                .stage(value)
                .commit();
            let key = current.key();
            // SAFETY: both owners retain count, and current is the native occupied source.
            let slot = unsafe { depot::raw::Proof::new(((&mut heap, current), key)) }.into();
            let current = slot.replace(4096 - value);
            assert_eq!(*current.get(), value);
            let (stored, priority) = heap.peek().expect("replacement remains scheduled");
            assert!(stored == key);
            assert_eq!(*priority, 4096 - value);
            let current = if value % 2 == 0 {
                // SAFETY: the returned native source retains the same paired physical slot.
                let slot = unsafe { depot::raw::Proof::new(((&mut heap, current), key)) }.into();
                let current = slot.clear();
                assert!(heap.is_empty());
                current
            } else {
                current
            };
            assert_eq!(current.remove(), value);
        }
    });
    assert_eq!(allocs, [0; 4]);
    assert_eq!(heap.len(), 1);
    assert_eq!(source.len(), o3::depot::capacity::Count::EMPTY);
}

#[test]
fn projects_scheduled() {
    assert!(
        keyed::Min::<key::Handle, u64>::try_with(capacity::Count::new(1), |mut heap, domain| {
            let mut source = round::Set::<u64, (), { u32::MAX }, _>::try_in(domain)
                .expect("resident allocation");
            let ((), allocs) = Tracker::measure(|| {
                for value in 0..4096 {
                    let key = {
                        let entry = source.try_vacant().expect("retired resident").insert(value);
                        let key = entry.key();
                        heap.replace(key, value + 7);
                        key
                    };
                    let retry = heap.first_entry().expect("scheduled resident");
                    assert!(retry.key() == key);
                    // SAFETY: this fixture co-owns both stores and retires the heap before its resident.
                    let (mut entry, retry) =
                        unsafe { depot::raw::Proof::new(((&mut source, retry), key)) }.into();
                    assert_eq!(retry.remove(), value + 7);
                    assert_eq!(*entry.get(), value);
                    *entry.get_mut() += 1;
                    match entry.state() {
                        round::set::State::Idle(entry) => {
                            assert_eq!(*entry.get(), value + 1);
                            entry.discard();
                        }
                        round::set::State::Ready(_) => panic!("resident was idle"),
                    }
                }
            });
            assert_eq!(allocs, [0; 4]);
            assert!(heap.is_empty());
            assert_eq!(source.len(), o3::depot::capacity::Count::EMPTY);
        })
        .is_ok(),
        "heap allocation"
    );
}

#[test]
fn retains_generations() {
    let count = capacity::Count::new(2);
    assert!(
        keyed::Min::<key::Handle, u64>::try_with(count, |mut heap, domain| {
            let mut records =
                round::Set::<u64, (), { u32::MAX }, _>::try_in(domain).expect("source storage");
            let first = records.try_vacant().expect("first vacant").insert(1);
            let first_id = first.key();
            first.insert();
            let second = records.try_vacant().expect("second vacant").insert(2);
            let other_id = second.key();
            second.insert();
            let ((), allocs) = Tracker::measure(|| {
                heap.replace(first_id, 10);
                heap.replace(other_id, 20);
                let entry = records.try_entry(first_id);
                let entry = entry
                    .map_err(|error| match error {
                        key::Stale::Free { key, actual } => ("free", *key.as_inner(), actual),
                        key::Stale::Reserved { key, actual } => {
                            ("reserved", *key.as_inner(), actual)
                        }
                        key::Stale::Retired { key, actual } => ("retired", *key.as_inner(), actual),
                        key::Stale::Generation { key, actual } => {
                            ("generation", *key.as_inner(), actual)
                        }
                    })
                    .expect("the occupied source must be available");
                let entry = round::set::Reuse::try_from(entry);
                let entry = entry
                    .map_err(|entry| *entry.key().as_inner())
                    .expect("the reusable source must be available");
                let (entry, prior) = entry.replace(3);
                assert_eq!(prior, 1);
                let next = entry.key();
                assert!(next != first_id);
                heap.replace(next, 30);
                let first = heap.first_entry().expect("minimum");
                assert!(first.key() == other_id);
                assert_eq!(first.remove(), 20);
                heap.retire(first_id);
                let last = heap.first_entry().expect("new generation retained");
                assert!(last.key() == next);
                assert_eq!(last.remove(), 30);
                assert!(heap.is_empty());
                assert_eq!(*entry.into_mut(), 3);
                assert!(records.contains(next));
                assert!(!records.contains(first_id));
                let error = {
                    let result = records.try_entry(first_id);
                    assert!(result.is_err(), "superseded source");
                    match result {
                        Err(error) => error,
                        Ok(_) => return,
                    }
                };
                assert!(matches!(error, key::Stale::Generation { key, actual }
                if key == first_id && actual == next.as_inner().generation()));
            });
            assert_eq!(allocs, [0; 4]);
        })
        .is_ok(),
        "heap storage"
    );
}

#[test]
fn preserves_layout() {
    assert_eq!(
        mem::size_of::<capacity::Key<'static, key::Handle>>(),
        mem::size_of::<key::Handle>()
    );
    assert_eq!(
        mem::size_of::<Option<capacity::Key<'static, key::Handle>>>(),
        mem::size_of::<Option<key::Handle>>()
    );
    assert_eq!(
        mem::size_of::<keyed::Min<key::Handle, u64, u32, capacity::Domain<'static>>>(),
        mem::size_of::<keyed::Min<key::Handle, u64>>()
    );
    assert_eq!(
        mem::size_of::<round::Set<u64, (), { u32::MAX }, capacity::Domain<'static>>>(),
        mem::size_of::<round::Set<u64>>()
    );
}

#[test]
fn classifies_bound_state() {
    keyed::Min::<key::Handle, u64>::try_with(capacity::Count::new(1), |_, domain| {
        let mut records = round::Set::<u64, (), 2, _>::try_in(domain).expect("source storage");
        let first = records.try_vacant().expect("first vacancy").insert(1);
        let key = first.key();
        first.discard();
        assert!(
            matches!(records.try_entry(key), Err(key::Stale::Free { key: seen, actual })
            if seen == key && actual.get() == 2)
        );
        mem::forget(records.try_vacant().expect("next reservation"));
        assert!(
            matches!(records.try_entry(key), Err(key::Stale::Reserved { key: seen, actual })
            if seen == key && actual.get() == 2)
        );

        let mut records = round::Set::<u64, (), 1, _>::try_in(domain).expect("retiring storage");
        let last = records.try_vacant().expect("last vacancy").insert(2);
        let key = last.key();
        last.discard();
        assert!(
            matches!(records.try_entry(key), Err(key::Stale::Retired { key: seen, actual })
            if seen == key && actual == key.as_inner().generation())
        );
    })
    .expect("heap storage");
}

#[test]
fn retains_identity() {
    let first = Address {
        index: 0,
        generation: NonZero::<u8>::MIN,
    };
    let next = Address {
        generation: first.generation.checked_add(1).expect("second generation"),
        ..first
    };
    let mut heap =
        keyed::Min::<Address, u8>::try_new(Count::new(1)).expect("one complete identity fits");
    let vacant = match heap.entry(first).expect("first physical index fits") {
        Entry::Vacant(vacant) => vacant,
        state => {
            assert!(matches!(state, Entry::Vacant(_)));
            return;
        }
    };
    vacant.insert(7);
    let entry = heap
        .first_entry()
        .expect("the complete identity is retained");
    assert_eq!(entry.key(), first);
    assert_eq!(*entry.priority(), 7);
    let conflict = match heap.entry(next).expect("same physical index fits") {
        Entry::Conflict(conflict) => conflict,
        state => {
            assert!(matches!(state, Entry::Conflict(_)));
            return;
        }
    };
    assert_eq!(conflict.requested(), next);
    assert_eq!(conflict.current(), first);
    let outside = Address { index: 1, ..next };
    let error = {
        let result = heap.entry(outside);
        assert!(result.is_err(), "outside the physical extent");
        match result {
            Err(error) => error,
            Ok(_) => return,
        }
    };
    assert_eq!(error.key(), outside);
    assert_eq!(error.capacity(), 1);
    let entry = heap
        .first_entry()
        .expect("a failed lookup preserves the occupant");
    assert_eq!(entry.key(), first);
    assert_eq!(entry.remove(), 7);
    assert!(heap.is_empty());
}
