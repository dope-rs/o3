use o3::depot::{
    array::Index,
    queue::{group::capacity, indexed::slab, tagged},
};

#[test]
fn transfer_fifo() {
    let source = Index::at::<0>();
    let target = Index::at::<1>();
    let mut entries = slab::Slab::<u8, (), 1, tagged::Hook, 2, _>::try_from(
        capacity::Count::<2>::try_from(2).expect("capacity"),
    )
    .expect("entries");
    let first = entries
        .vacant()
        .expect("slot")
        .select(source)
        .insert(1)
        .key();
    let second = entries
        .vacant()
        .expect("slot")
        .select(target)
        .insert(2)
        .key();
    entries.try_entry(first).expect("member").move_back(target);
    assert!(entries.front_pair(source).is_none());
    assert_eq!(entries.front_pair(target), Some((second, &2)));
    assert_eq!(entries.try_get(first), Ok(&1));
    entries.try_entry(second).expect("member").move_back(target);
    assert_eq!(entries.front_pair(target), Some((first, &1)));
    for expected in [first, second] {
        let ((), entry) = entries.select([(target, ())]).expect("front");
        assert_eq!(entry.key(), expected);
        entry.remove();
    }
    assert!(entries.is_empty());
}
