use o3::depot::{capacity, queue::round};

#[test]
fn values_include_idle() {
    let mut set = round::Set::<u32>::try_new(capacity::Count::new(2)).expect("allocate two slots");
    set.try_vacant().expect("first slot available").insert(1);
    set.try_vacant()
        .expect("second slot available")
        .insert(2)
        .insert();

    assert!(set.values().copied().eq([1, 2]));
}
