use o3::depot::{
    queue::{links, round},
    raw,
};

#[test]
fn preserves_rotation() {
    let keys = [0, 1, 2].map(|index| links::Index::try_new(index).expect("index"));
    let hooks = keys.map(|index| raw::Node::vacant((), index.raw(), u32::MAX));
    let lanes = [keys[0], keys[1]].map(|index| raw::Node::vacant((), index.raw(), u32::MAX));
    let entries = [links::Chain::new(), links::Chain::new()];
    let active = links::Cycle::new();
    for (key, lane) in [(keys[0], 0), (keys[1], 0), (keys[2], 1)] {
        let join = round::Join {
            hooks: hooks.as_slice(),
            lanes: lanes.as_slice(),
            entries: &entries[lane],
            active: &active,
            index: keys[lane],
        };
        // SAFETY: the hook is vacant in these disjoint stable stores, and each
        // completed insertion preserves exact nonempty active membership.
        round::Join::insert(unsafe { raw::Proof::new((join, key)) });
    }
    for (lane, expected) in [(0, keys[0]), (1, keys[2]), (0, keys[1])] {
        assert_eq!(active.first(), Some(keys[lane]));
        let join = round::Join {
            hooks: hooks.as_slice(),
            lanes: lanes.as_slice(),
            entries: &entries[lane],
            active: &active,
            index: keys[lane],
        };
        // SAFETY: the asserted active front retains its nonempty FIFO until
        // this consuming removal; neither store is changed between them.
        let removed = round::Join::active::<true>(unsafe { raw::Proof::new(join) });
        assert_eq!(removed.index(), expected);
        removed.idle();
    }
    assert!(active.is_empty());
}
