use o3::depot::index::{self, bitmap::Fill, set};

#[test]
fn clears_planes_and_reuses() {
    let domain = index::Domain::try_new(65).expect("domain");
    let sets = set::State::<(), usize, 3>::try_new(domain, [Fill::Full; 3]).expect("sets");
    let row = sets.try_entry(index::Ordinal::new(64)).expect("last row");

    sets.clear();

    for id in [set::Id::at::<0>(), set::Id::at::<1>(), set::Id::at::<2>()] {
        assert!(sets.is_empty(id));
    }
    row.set(set::Id::at::<1>());
    let entry = sets
        .get::<1>()
        .pop_from(index::Ordinal::new(0))
        .expect("reused row");
    assert_eq!(entry.index().raw(), 64);
    assert!(sets.get::<1>().is_empty());
}
