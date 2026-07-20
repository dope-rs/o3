use o3::collections::{CellSlab, Slab, SlabKeyParts};

#[test]
fn generational_reuse_and_capacity() {
    let mut slab: Slab<&str> = Slab::with_capacity(3);
    let first = slab.insert("a").unwrap();
    let recycled = slab.insert("b").unwrap();
    let last = slab.insert("c").unwrap();
    assert!(slab.is_full());
    assert!(slab.insert("overflow").is_err());

    assert_eq!(slab.remove(recycled), Some("b"));
    let replacement = slab.insert("B").unwrap();
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
fn entry_and_explicit_index_paths_preserve_the_free_list() {
    let mut slab = Slab::<u32>::with_capacity(200);
    let entry = slab.vacant_entry().unwrap();
    let first = entry.insert(1);
    let (second, value) = slab.insert_entry(2).unwrap();
    *value = 3;
    assert_eq!(slab.get(first), Some(&1));
    assert_eq!(slab.get(second), Some(&3));

    let high = slab.insert_at_with(130, |key| key.index()).unwrap();
    let middle = slab.insert_at_with(70, |key| key.index()).unwrap();
    assert!(slab.insert_at_with(70, |_| 0).is_none());
    let next = slab.insert(4).unwrap();
    assert_eq!(next.index(), 2);

    let (value, indexed) = slab.get_index(130).unwrap();
    assert_eq!((*value, indexed), (130, high));
    let (value, indexed) = slab.get_index_mut(70).unwrap();
    *value += 1;
    assert_eq!(indexed, middle);
    assert_eq!(slab.get(middle), Some(&71));

    assert_eq!(slab.remove(high), Some(130));
    let replacement = slab.insert_at_with(130, |_| 7).unwrap();
    assert_ne!(high, replacement);
    assert_eq!(slab.get(replacement), Some(&7));
}

#[test]
fn sparse_iteration_clear_and_index_removal_follow_live_entries() {
    let mut slab: Slab<u32> = Slab::with_capacity(128);
    assert_eq!(slab.key(127), None);
    let high = slab.insert_at_with(127, |_| 7).unwrap();
    let low = slab.insert_at_with(1, |_| 3).unwrap();
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
    assert_eq!(slab.remove_index(low.index()), None);

    let live = slab.insert(9).unwrap();
    slab.clear();
    assert!(slab.is_empty());
    assert_eq!(slab.key(live.index()), None);
}

#[test]
fn growth_preserves_live_retired_and_dense_entries() {
    let mut slab = Slab::<u8, (), 1>::with_capacity(2);
    let retired = slab.insert(1).unwrap();
    let live = slab.insert(2).unwrap();
    assert_eq!(slab.remove(retired), Some(1));
    slab.grow_to(4);
    assert_eq!(slab.capacity(), 4);
    assert_eq!(slab.get(live), Some(&2));
    assert_eq!(slab.get(retired), None);
    let first = slab.insert(3).unwrap();
    let second = slab.insert(4).unwrap();
    assert_ne!(first.index(), retired.index());
    assert_ne!(second.index(), retired.index());
    assert!(slab.insert(5).is_err());

    let mut slab: CellSlab<i32> = CellSlab::with_capacity(1);
    let first = slab.insert(7).unwrap();
    slab.grow_to(3);
    let second = slab.insert(8).unwrap();
    let third = slab.insert(9).unwrap();
    assert_eq!(slab.update(first, |value| *value), Some(7));
    assert_eq!(slab.update(second, |value| *value), Some(8));
    assert_eq!(slab.update(third, |value| *value), Some(9));
    assert_eq!(slab.keys().count(), 3);
}

#[test]
fn external_parts_resolve_only_the_current_generation() {
    const MAX: u32 = 7;
    struct Tag;

    assert!(SlabKeyParts::<MAX>::new(0, 0).is_none());
    assert!(SlabKeyParts::<MAX>::new(0, MAX + 1).is_none());
    assert!(SlabKeyParts::<MAX>::new(u32::MAX, MAX).is_some());

    let mut slab = Slab::<u32, Tag, MAX>::with_capacity(1);
    let key = slab.insert(7).unwrap();
    let parts = SlabKeyParts::<MAX>::new(key.index(), key.generation().get()).unwrap();
    assert_eq!(slab.get_parts(parts), Some(&7));
    assert_eq!(slab.resolve(parts), Some(key));
    assert_eq!(slab.remove_parts(parts), Some(7));
    assert_eq!(slab.get_parts(parts), None);

    let slab = CellSlab::<u32, Tag, MAX>::with_capacity(1);
    let stale = slab.insert(7).unwrap().parts();
    assert_eq!(slab.remove_parts(stale), Some(7));
    let current = slab.insert(11).unwrap().parts();
    assert_eq!(slab.update_parts(stale, |value| *value += 1), None);
    assert_eq!(slab.update_parts(current, |value| *value += 1), Some(()));
    assert_eq!(slab.remove_parts(current), Some(12));
}
