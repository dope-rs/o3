use o3::collections::slab::{
    self,
    external::{self, Cell, Exclusive},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Generation(u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WideGeneration(u32);

impl external::Generation for Generation {
    const INITIAL: Self = Self(1);

    fn next(self) -> Option<Self> {
        (self.0 < 3).then(|| Self(self.0 + 1))
    }
}

impl external::Generation for WideGeneration {
    const INITIAL: Self = Self(1);

    fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

#[test]
fn partition_heads_reserve_and_recycle_without_crossing_ranges() {
    let mut slab = Exclusive::<u32, WideGeneration, (), { u32::MAX }, 2>::with_capacity(
        slab::Capacity::new(8),
    );

    let cancelled = slab.vacant_entry_in(0..4).unwrap();
    assert_eq!(cancelled.key().index(), 0);
    drop(cancelled);

    let mut primary = Vec::new();
    for expected in 0..4 {
        let entry = slab.vacant_entry_in(0..4).unwrap();
        assert_eq!(entry.key().index(), expected);
        primary.push(entry.insert(expected));
    }
    assert!(slab.vacant_entry_in(0..4).is_none());

    let secondary = slab.vacant_entry_in(4..8).unwrap();
    assert_eq!(secondary.key().index(), 4);
    let secondary = secondary.insert(4);
    assert_eq!(slab.vacant_entry_in(0..8).unwrap().key().index(), 5);

    let recycled = primary.swap_remove(2);
    assert_eq!(slab.remove(recycled), Some(2));
    assert_eq!(slab.vacant_entry_in(0..4).unwrap().key().index(), 2);
    assert_eq!(slab.remove(secondary), Some(4));
}

#[test]
fn logical_generations_reject_stale_keys_across_physical_wrap() {
    let mut slab = Exclusive::<u32, Generation, (), 2>::with_capacity(slab::Capacity::new(1));

    let first = slab.insert(1).unwrap();
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.insert(2).unwrap();
    assert_eq!(slab.remove(second), Some(2));
    let third = slab.insert(3).unwrap();

    assert_eq!(first.generation(), Generation(1));
    assert_eq!(second.generation(), Generation(2));
    assert_eq!(third.generation(), Generation(3));
    assert_eq!(first.index(), third.index());
    assert_eq!(slab.get(first), None);
    assert_eq!(slab.get(second), None);
    assert_eq!(slab.get(third), Some(&3));
    assert_eq!(slab.remove(third), Some(3));
    assert!(slab.insert(4).is_err());
    assert_eq!(slab.available(), 1);
}

#[test]
fn exclusive_reservation_rollback_consumes_one_logical_generation() {
    let mut slab = Exclusive::<u32, Generation>::with_capacity(slab::Capacity::new(2));
    let cancelled = slab.vacant_entry_at(1).unwrap();
    assert_eq!(cancelled.key().generation(), Generation(1));
    drop(cancelled);

    let current = slab.vacant_entry_at(1).unwrap().insert(7);
    assert_eq!(current.generation(), Generation(2));
    assert_eq!(slab.entries().current(1), Some((&7, current)));
    *slab.entries_mut().get(current).unwrap() = 9;
    assert_eq!(slab.occupied_entry(current).unwrap().get(), &9);
}

#[test]
fn cell_preserves_full_reject_and_reentrant_busy_semantics() {
    let slab = Cell::<u32, Generation>::with_capacity(slab::Capacity::new(1));
    let first = slab.insert(1).unwrap();

    // The existing external-identity contract consumes an attempted identity
    // before discovering that the interior slab is full.
    assert_eq!(slab.insert(2), Err(2));
    assert_eq!(slab.remove(first), Some(1));
    let third = match slab.try_insert_with(3, |key, value| {
        assert_eq!(key.generation(), Generation(3));
        assert_eq!(slab.update(key, |_| ()), None);
        assert!(slab.any_or_busy(|_| false));
        *value = 4;
        Ok::<_, ()>(5)
    }) {
        Ok(inserted) => inserted,
        Err(_) => panic!("third logical identity should commit"),
    };
    assert_eq!(third.1, 5);
    assert_eq!(slab.key_at(0), Some(third.0));
    assert_eq!(slab.keys().collect::<Vec<_>>(), [third.0]);
    assert_eq!(slab.update(third.0, |value| *value), Some(4));
}
