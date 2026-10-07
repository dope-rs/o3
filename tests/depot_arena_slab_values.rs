use o3::depot::{arena::slab, capacity};

#[test]
fn values_skip_removed() {
    let mut store =
        slab::Store::<u32>::try_new(capacity::Count::new(3)).expect("allocate three slots");
    let removed = store.insert(1).expect("first slot available");
    store.insert(2).expect("second slot available");
    store.remove(removed);

    assert!(store.values().copied().eq([2]));
}
