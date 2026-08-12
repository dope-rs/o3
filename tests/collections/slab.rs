use o3::collections::slab::{Capacity, Cell, Slab, key::Parts};

#[test]
fn capacity_preserves_its_exact_index_width() {
    assert_eq!(Capacity::EMPTY.raw(), 0);
    assert_eq!(Capacity::MAX.raw(), u32::MAX);
    assert_eq!(Capacity::new(17).raw(), 17);
}

#[test]
fn cell_transactions_commit_reject_and_rollback_without_exposing_busy_values() {
    use o3::collections::slab::{BuildError, InsertError};

    let slab = Cell::<u32>::with_capacity(Capacity::new(1));
    let inserted = slab.try_insert_with(3, |key, value| {
        assert_eq!(slab.remove(key), None);
        assert!(slab.any_or_busy(|_| false));
        *value = 5;
        Ok::<_, ()>(7)
    });
    let (key, result) = match inserted {
        Ok(inserted) => inserted,
        Err(_) => panic!("transaction should commit"),
    };
    assert_eq!(result, 7);
    assert_eq!(slab.update(key, |value| *value), Some(5));
    assert!(slab.any_or_busy(|value| *value == 5));
    assert_eq!(slab.remove(key), Some(5));

    let rejected = slab.try_insert_with(11, |_, value| {
        *value = 13;
        Err::<(), _>(17)
    });
    match rejected {
        Err(InsertError::Rejected(value, error)) => assert_eq!((value, error), (13, 17)),
        _ => panic!("transaction should reject"),
    }
    assert!(slab.is_empty());

    let built = slab.try_insert_build(19, |input| input + 1, |_, _| Err::<(), _>(23));
    match built {
        Err(BuildError::Rejected(value, error)) => assert_eq!((value, error), (20, 23)),
        _ => panic!("built transaction should reject"),
    }

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = slab.try_insert_with(29, |_, _| -> Result<(), ()> {
            panic!("rollback");
        });
    }));
    assert!(caught.is_err());
    assert!(slab.is_empty());
    assert_eq!(slab.available(), 1);
}

#[test]
fn generational_reuse_and_capacity() {
    let mut slab: Slab<&str> = Slab::with_capacity(Capacity::new(3));
    assert_eq!(slab.available(), 3);
    let first = slab.insert("a").unwrap();
    let recycled = slab.insert("b").unwrap();
    let last = slab.insert("c").unwrap();
    assert_eq!(slab.available(), 0);
    assert!(slab.is_full());
    assert!(slab.insert("overflow").is_err());

    assert_eq!(slab.remove(recycled), Some("b"));
    assert_eq!(slab.available(), 1);
    let replacement = slab.insert("B").unwrap();
    assert_eq!(slab.available(), 0);
    assert_eq!(replacement.index(), recycled.index());
    assert_ne!(replacement.generation(), recycled.generation());
    assert_eq!(slab.get(recycled), None);
    assert_eq!(slab.remove(recycled), None);
    assert_eq!(slab.get(replacement), Some(&"B"));

    *slab.get_mut(last).unwrap() = "C";
    assert_eq!(slab.get(first), Some(&"a"));
    assert_eq!(slab.get(last), Some(&"C"));
}

#[test]
fn private_generations_can_wrap_for_a_wider_identity_wrapper() {
    let capacity = Capacity::new(1);
    // SAFETY: this test never treats the returned physical keys as an external
    // stale-identity authority; it only verifies private slot availability.
    let mut slab = unsafe {
        Slab::<u32, (), 2, true>::try_with_capacity_recycling(capacity).expect("recycling slab")
    };

    let first = slab.insert(1).expect("generation one");
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.insert(2).expect("generation two");
    assert_eq!(slab.remove(second), Some(2));
    let wrapped = slab.insert(3).expect("wrapped generation one");
    assert_eq!(wrapped.generation().get(), 1);
    assert_eq!(slab.remove(wrapped), Some(3));
}

#[test]
fn entry_and_explicit_index_paths_preserve_the_free_list() {
    let mut slab = Slab::<u32>::with_capacity(Capacity::new(200));
    let entry = slab.vacant_entry().unwrap();
    let first = entry.insert(1);
    let (second, value) = slab.insert_entry(2).unwrap();
    *value = 3;
    assert_eq!(slab.get(first), Some(&1));
    assert_eq!(slab.get(second), Some(&3));

    let high_entry = slab.vacant_entry_at(130).unwrap();
    let high_value = high_entry.key().index();
    let high = high_entry.insert(high_value);
    let middle_entry = slab.vacant_entry_at(70).unwrap();
    let middle_value = middle_entry.key().index();
    let middle = middle_entry.insert(middle_value);
    assert!(slab.vacant_entry_at(70).is_none());
    let next = slab.insert(4).unwrap();
    assert_eq!(next.index(), 2);

    let (value, indexed) = slab.get_index(130).unwrap();
    assert_eq!((*value, indexed), (130, high));
    let (value, indexed) = slab.get_index_mut(70).unwrap();
    *value += 1;
    assert_eq!(indexed, middle);
    assert_eq!(slab.get(middle), Some(&71));

    assert_eq!(slab.remove(high), Some(130));
    let replacement = slab.vacant_entry_at(130).unwrap().insert(7);
    assert_ne!(high, replacement);
    assert_eq!(slab.get(replacement), Some(&7));
}

#[test]
fn range_reservation_never_escapes_its_partition() {
    let mut slab = Slab::<u32>::with_capacity(Capacity::new(6));
    let occupied = slab.vacant_entry_at(2).unwrap().insert(2);
    let occupied_next = slab.vacant_entry_at(3).unwrap().insert(3);

    let selected = slab.vacant_entry_in(2..5).expect("free slot in range");
    assert_eq!(selected.key().index(), 4);
    let selected = selected.insert(4);
    assert_eq!(slab.get(selected), Some(&4));

    assert!(slab.vacant_entry_in(2..4).is_none());
    assert!(slab.vacant_entry_in(5..5).is_none());
    assert_eq!(slab.vacant_entry_in(5..6).unwrap().key().index(), 5);

    assert_eq!(slab.remove(occupied), Some(2));
    assert_eq!(slab.remove(occupied_next), Some(3));
}

#[test]
fn occupied_entry_carries_exclusive_generation_proof_through_removal() {
    let mut slab = Slab::<u32>::with_capacity(Capacity::new(2));
    let key = slab.insert(7).expect("first slot");

    {
        let mut entry = slab
            .occupied_entry_parts(key.parts())
            .expect("current generation");
        assert_eq!(entry.key(), key);
        assert_eq!(entry.get(), &7);
        *entry.get_mut() = 11;
    }
    assert_eq!(slab.get(key), Some(&11));

    let entry = slab.occupied_entry_at(key.index()).expect("occupied index");
    assert_eq!(entry.remove(), 11);
    assert!(slab.occupied_entry(key).is_none());

    let replacement = slab.insert(13).expect("recycled slot");
    assert_eq!(replacement.index(), key.index());
    assert_ne!(replacement.generation(), key.generation());
    assert!(slab.occupied_entry(key).is_none());
    assert_eq!(slab.occupied_entry(replacement).unwrap().get(), &13);

    let inserted = slab
        .vacant_entry_at(1)
        .expect("second slot")
        .insert_occupied(17);
    assert_eq!(inserted.get(), &17);
    let inserted_key = inserted.key();
    drop(inserted);
    assert_eq!(slab.get(inserted_key), Some(&17));
}

#[test]
fn indexed_entry_reserves_before_construction_and_rolls_back_on_drop() {
    let mut slab = Slab::<u32>::with_capacity(Capacity::new(4));
    let occupied = slab.vacant_entry_at(1).unwrap().insert(7);
    assert!(slab.vacant_entry_at(occupied.index()).is_none());
    assert!(slab.vacant_entry_at(4).is_none());

    let reservation = slab.vacant_entry_at(3).unwrap();
    let cancelled = reservation.key();
    assert_eq!(cancelled.index(), 3);
    drop(reservation);

    let replacement = slab.vacant_entry_at(3).unwrap().insert(9);
    assert_ne!(replacement, cancelled);
    assert_eq!(slab.get(replacement), Some(&9));
    assert_eq!(slab.insert(11).unwrap().index(), 0);
}

#[test]
fn sparse_iteration_clear_and_index_removal_follow_live_entries() {
    let mut slab: Slab<u32> = Slab::with_capacity(Capacity::new(128));
    assert_eq!(slab.key(127), None);
    let high = slab.vacant_entry_at(127).unwrap().insert(7);
    let low = slab.vacant_entry_at(1).unwrap().insert(3);
    assert_eq!(slab.values().copied().collect::<Vec<_>>(), [7, 3]);
    assert_eq!(slab.remove(high), Some(7));
    assert_eq!(slab.values().copied().collect::<Vec<_>>(), [3]);

    let (value, generation) = slab
        .remove_index_with(low.index(), |value, key| {
            *value += 1;
            Some(key.generation())
        })
        .unwrap();
    assert_eq!(value, 4);
    assert_eq!(generation, low.generation());

    let live = slab.insert(9).unwrap();
    slab.clear();
    assert!(slab.is_empty());
    assert_eq!(slab.key(live.index()), None);
}

#[test]
fn cell_slab_growth_preserves_live_entries() {
    let mut slab: Cell<i32> = Cell::with_capacity(Capacity::new(1));
    let first = slab.insert(7).unwrap();
    slab.grow_to(Capacity::new(3));
    let second = slab.insert(8).unwrap();
    let third = slab.insert(9).unwrap();
    assert_eq!(slab.update(first, |value| *value), Some(7));
    assert_eq!(slab.update(second, |value| *value), Some(8));
    assert_eq!(slab.update(third, |value| *value), Some(9));
    assert_eq!(slab.keys().count(), 3);
}

#[test]
fn capacity_proof_bounds_construction_and_growth() {
    assert_eq!(
        Capacity::try_from(u32::MAX as usize).unwrap(),
        Capacity::MAX
    );
    if usize::BITS > 32 {
        let error = Capacity::try_from(u32::MAX as usize + 1).unwrap_err();
        assert_eq!(error.requested(), u32::MAX as usize + 1);
    }

    let slab = Cell::<u8>::with_capacity(Capacity::new(2));
    assert_eq!(slab.capacity(), 2);
}

#[test]
fn external_parts_resolve_only_the_current_generation() {
    const MAX: u32 = 7;
    struct Tag;

    assert!(Parts::<MAX>::new(0, 0).is_none());
    assert!(Parts::<MAX>::new(0, MAX + 1).is_none());
    assert!(Parts::<MAX>::new(u32::MAX, MAX).is_some());

    let mut slab = Slab::<u32, Tag, MAX>::with_capacity(Capacity::new(1));
    let key = slab.insert(7).unwrap();
    let parts = Parts::<MAX>::new(key.index(), key.generation().get()).unwrap();
    assert_eq!(slab.get_parts(parts), Some(&7));
    assert_eq!(slab.remove_parts(parts), Some(7));
    assert_eq!(slab.get_parts(parts), None);

    let slab = Cell::<u32, Tag, MAX>::with_capacity(Capacity::new(1));
    let stale = slab.insert(7).unwrap().parts();
    assert_eq!(slab.remove_parts(stale), Some(7));
    let current = slab.insert(11).unwrap().parts();
    assert_eq!(slab.update_parts(stale, |value| *value += 1), None);
    assert_eq!(slab.update_parts(current, |value| *value += 1), Some(()));
    assert_eq!(slab.remove_parts(current), Some(12));
}
