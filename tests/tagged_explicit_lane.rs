use o3::depot::{
    array::Index,
    capacity::Limit,
    index::Initialized,
    queue::{indexed::lanes, links, tagged},
    raw::Proof,
};

#[test]
fn returns_owner_after_front_transfer() {
    let mut fifo = lanes::Fifo::<u8, 2, _>::try_from(Limit::<2>::fixed::<2>()).expect("two slots");
    fifo.vacant::<0>(0)
        .expect("source")
        .insert::<false>(10)
        .commit();
    fifo.vacant::<1>(1)
        .expect("target")
        .insert::<false>(20)
        .commit();
    let index = links::Index::try_new(0).expect("source index");
    // SAFETY: the exclusive owner retains this initialized member and its hook.
    let entry = unsafe { Proof::<_, Initialized<links::Chain>>::new((&mut fifo, index)) };
    let entry = tagged::Vacant::detach(entry)
        .push_at::<true, _>(Index::at::<1>())
        .into_inner();
    let (owner, returned) = entry.into_inner();
    assert_eq!(returned, index);
    assert!(owner.pop_front::<0>().is_none());
    assert_eq!(owner.pop_front::<1>(), Some(10));
}
