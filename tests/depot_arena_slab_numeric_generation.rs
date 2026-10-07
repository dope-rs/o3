pub mod common;

use common::depot::arena::slab::external::Generation;
use o3::{
    base::num,
    depot::{
        arena::slab::external::{Exclusive, errors::Lookup, shared::Storage},
        capacity::Count,
    },
};

#[test]
fn wraps_physical_epoch() {
    let slab = Storage::<u32, Generation, (), 1>::try_new(Count::new(1))
        .expect("one physical generation remains recyclable");
    let mut key = slab.vacancy().expect("initial vacancy").insert(1);
    for epoch in 2..=3 {
        let old = key;
        let entry = slab.try_entry(key).expect("current logical identity");
        let value = entry.take();
        let vacant = slab
            .vacancy()
            .expect("next logical identity survives physical wrap");
        assert_eq!(vacant.key().index(), old.index());
        assert_eq!(vacant.key().generation().get(), epoch);
        key = vacant.insert(value + 1);
        assert!(matches!(
            slab.try_entry(old),
            Err(Lookup::Generation { key: stale, actual })
                if stale == old && actual.get() == epoch
        ));
    }
    let value = slab.try_entry(key).expect("last identity").take();
    assert_eq!(value, 3);
    assert!(slab.is_empty());
    assert!(matches!(
        slab.vacancy(),
        Err(o3::depot::arena::slab::Vacancy::IdentityExhausted)
    ));
}

#[test]
fn exhausts_numeric_keys() {
    let mut slab = Exclusive::<u32, num::Generation<1>, (), 1>::try_new(Count::new(1))
        .expect("one slot must allocate");
    let first = slab
        .try_grant()
        .expect("zero is a valid initial identity")
        .reserve();
    let old = first.key();
    assert_eq!(old.generation().get(), 0);
    first.commit(10);
    assert_eq!(slab.occupied(old).map(|entry| entry.remove()), Some(10));

    let last = slab
        .try_grant()
        .expect("the final identity must remain available")
        .reserve();
    let current = last.key();
    assert_eq!(current.generation().get(), 1);
    assert_eq!(old.index(), current.index());
    last.commit(20);
    assert_eq!(slab.get(old), None);
    assert_eq!(slab.get(current), Some(&20));
    assert_eq!(slab.occupied(current).map(|entry| entry.remove()), Some(20));
    assert!(matches!(
        slab.try_grant(),
        Err(o3::depot::arena::slab::Vacancy::IdentityExhausted)
    ));
}
