use std::{array, mem};

use o3::depot::{
    arena::slab::{self, Store, key},
    capacity::Count,
};

#[test]
fn retains_partition_order() {
    let mut slab = Store::<u32, (), 7, false, 2>::try_new(Count::new(4)).expect("four slots");
    let old = slab.insert(10).expect("first slot");
    slab.remove(old);

    let mut entries = slab.vacancies();
    let slots: [slab::Vacant<_, (), false>; 4] =
        array::from_fn(|_| entries.next().expect("four free slots"));
    assert!(entries.next().is_none());
    drop(entries);
    let keys = slots.map(|entry| entry.insert(20));
    assert_eq!(keys.map(|key| key.index()), [0, 1, 2, 3]);
    assert_eq!(keys.map(|key| key.generation().get()), [2, 1, 1, 1]);
}

#[test]
fn excludes_unavailable() {
    let mut slab = Store::<u32, (), 1>::try_new(Count::new(3)).expect("three slots");
    let retired = slab.insert(10).expect("first slot");
    slab.remove(retired);
    mem::forget(slab.try_vacant().expect("second slot"));

    let mut entries = slab.vacancies();
    let last = entries.next().expect("only the third slot is free");
    assert!(entries.next().is_none());
    drop(last);
    let last = entries.next().expect("unused reservation returned");
    drop(entries);
    let key = last.insert(30);
    assert_eq!((key.index(), key.generation().get()), (2, 1));
    assert!(slab.vacancies().next().is_none());
}

#[test]
fn retains_disjoint_slots() {
    let mut slab = Store::<u32>::try_new(Count::new(3)).expect("three slots");
    let mut entries = slab.vacancies();
    let [first, second, third] =
        array::from_fn(|_| entries.next().expect("three distinct vacancies"));
    drop(entries);
    let mut first = first.stage(10).commit();
    let mut second = second.stage(20).commit();
    let keys = [first.key(), second.key()];
    let values = [first.get_mut(), second.get_mut()];
    *values[0] += 1;
    *values[1] += 2;
    drop((first, second, third));

    assert_eq!(
        keys.map(|key| slab.try_get(key).ok().copied()),
        [Some(11), Some(22)]
    );
    let key = slab
        .try_vacant()
        .expect("unused reservation returned")
        .insert(30);
    assert_eq!((key.index(), key.generation().get()), (2, 1));
}

#[test]
fn reserved_identity() {
    let slab = slab::Cell::<u32, (), 1>::try_new(Count::new(2)).expect("two identities");
    let first = slab.try_vacancy().expect("first reservation");
    let second = slab.try_vacancy().expect("second reservation");
    assert!(matches!(slab.try_vacancy(), Err(slab::Vacancy::Full)));
    assert_eq!(slab.remaining(), Count::new(2));
    assert!(slab.is_empty());

    drop(first);
    second.reject();
    assert_eq!(slab.remaining(), Count::new(1));
    let key = slab
        .try_vacancy()
        .expect("unissued identity returns")
        .insert(7);
    assert!(matches!(slab.try_vacancy(), Err(slab::Vacancy::Full)));
    assert_eq!(slab.try_entry(key).expect("last value").remove(), 7);
    assert_eq!(slab.remaining(), Count::EMPTY);
    assert!(matches!(
        slab.try_vacancy(),
        Err(slab::Vacancy::IdentityExhausted)
    ));
}

#[test]
fn remaining_capacity() {
    let mut empty = Store::<u32, (), 1>::try_new(Count::EMPTY).expect("empty extent");
    assert!(matches!(empty.try_vacant(), Err(slab::Vacancy::Full)));

    let mut slab = Store::<u32, (), 1>::try_new(Count::new(2)).expect("two identities");
    assert_eq!(slab.try_ready(), Ok(()));
    let first = slab.try_vacant().expect("first slot").insert(1);
    assert_eq!(slab.remove(first), Some(1));
    assert_eq!(slab.remaining(), Count::new(1));
    slab.try_vacant().expect("remaining slot").insert(2);
    assert_eq!(slab.try_ready(), Err(slab::Vacancy::Full));
    assert!(matches!(slab.try_vacant(), Err(slab::Vacancy::Full)));
    slab.clear();
    assert_eq!(slab.capacity(), 2);
    assert_eq!(slab.try_ready(), Err(slab::Vacancy::IdentityExhausted));
    assert_eq!(slab.remaining(), Count::EMPTY);
    assert!(matches!(
        slab.try_vacant(),
        Err(slab::Vacancy::IdentityExhausted)
    ));
}

#[test]
fn issued_cancel_advances() {
    let slab = slab::Cell::<u8, (), 2>::try_new(Count::new(1)).expect("one slot");
    let issued = slab.try_vacancy().expect("first identity").reserve();
    let stale = issued.key();
    drop(issued);

    let issued = slab.try_vacancy().expect("next identity").reserve();
    let fresh = issued.key();
    assert_eq!(fresh.index(), stale.index());
    let entry = issued.occupy(7);
    assert_eq!(entry.key(), fresh);
    drop(entry);
    assert!(matches!(
        slab.try_entry(stale),
        Err(key::Error::Generation { key, actual })
            if key == stale && actual == fresh.generation()
    ));
    assert_eq!(slab.update(fresh, |value| *value), Some(7));
}

#[test]
fn issued_cancel_exhausts() {
    let slab = slab::Cell::<u8, (), 1>::try_new(Count::new(1)).expect("one identity");
    drop(slab.try_vacancy().expect("unissued reservation"));
    let issued = slab.try_vacancy().expect("unspent identity").reserve();
    assert_eq!(issued.key().generation().get(), 1);
    drop(issued);
    assert!(matches!(
        slab.try_vacancy(),
        Err(slab::Vacancy::IdentityExhausted)
    ));
}
