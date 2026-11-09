pub mod common;

#[global_allocator]
static ALLOCATOR: common::alloc::system::Tracker = common::alloc::system::Tracker;

use std::{cell::Cell, mem, pin::pin, ptr};

use common::depot::arena::slab::{
    Short,
    external::{Generation, Pinned, Wide},
};
use o3::depot::{
    arena::slab::{
        self,
        external::{self, Exclusive, errors::Lookup, shared::Storage},
        lookup,
    },
    capacity::{self, Count},
    index::Initialized,
    raw::Proof,
};

#[test]
fn pinned_issuance() {
    let drops = Cell::new(0);
    let ((), counts) = common::alloc::system::Tracker::counts(|| {
        let mut owner = pin!(
            external::Pinned::<_, Generation>::try_new(Count::new(1)).expect("one physical slot")
        );
        let mut owner = std::hint::black_box(owner.as_mut());
        let initial = external::Key::new(0, Generation::MIN);
        assert!(matches!(
            owner.as_mut().try_occupied(initial),
            Err(Lookup::Physical {
                cause: lookup::Error::Free { index: 0 },
                ..
            })
        ));
        assert!(matches!(
            owner.as_mut().try_occupied(external::Key::new(1, Generation::MIN)),
            Err(Lookup::Physical { cause: lookup::Error::Index { index: 1, capacity }, .. })
                if capacity == Count::new(1)
        ));
        drop(owner.as_mut().try_grant().expect("cancelled preparation"));
        let grant = owner.as_mut().try_grant().expect("unspent identity");
        assert_eq!(grant.index(), 0);
        let (first, value) = grant.occupy(Pinned::new(&drops));
        let address = ptr::from_ref(value.as_ref().get_ref());
        value.address.set(address);
        assert_eq!(first, initial);
        assert!(matches!(
            owner.as_mut().try_grant(),
            Err(slab::Vacancy::Full)
        ));
        let value = owner
            .as_mut()
            .try_occupied(first)
            .expect("current identity")
            .into_value();
        assert!(ptr::eq(value.as_ref().get_ref(), address));
        owner
            .as_mut()
            .try_occupied(first)
            .expect("retained identity")
            .discard();
        assert_eq!(drops.get(), 1);

        let (second, value) = owner
            .as_mut()
            .try_grant()
            .expect("recycled slot")
            .occupy(Pinned::new(&drops));
        value.address.set(ptr::from_ref(value.as_ref().get_ref()));
        assert_eq!(second.generation().get(), 2);
        assert!(matches!(
            owner.as_mut().try_occupied(first),
            Err(Lookup::Generation { key, actual }) if key == first && actual == second.generation()
        ));
        // SAFETY: second's index is initialized in this same retained owner.
        let live =
            unsafe { Proof::<_, Initialized<()>>::new((owner.as_ref().get_ref(), second.index())) };
        let value = live.into();
        assert!(ptr::eq(value, address));
        // SAFETY: the same live index remains under this owner's exclusive pinned loan.
        let live = unsafe { Proof::<_, Initialized<()>>::new((owner.as_mut(), second.index())) };
        let mut entry = live.into();
        assert!(ptr::eq(entry.value().as_ref().get_ref(), address));
        entry.discard();
        assert_eq!(drops.get(), 2);

        let (last, value) = owner
            .as_mut()
            .try_grant()
            .expect("last logical identity")
            .occupy(Pinned::new(&drops));
        value.address.set(ptr::from_ref(value.as_ref().get_ref()));
        assert_eq!(last.generation().get(), 3);
        assert!(matches!(
            owner.as_mut().try_grant(),
            Err(slab::Vacancy::IdentityExhausted)
        ));
    });
    assert_eq!(counts, [1, 1]);
    assert_eq!(drops.get(), 3);
}

#[test]
fn partition_ranges() {
    let mut slab = (Exclusive::<u32, Wide, (), 2>::try_new(capacity::Count::new(8)))
        .expect("eight partitioned slab slots must fit");

    {
        let free =
            (slab.try_grant_in(0..4)).expect("the empty first partition must have a vacancy");
        let free = free.reserve();
        assert_eq!(free.key().index(), 0);
    }

    let mut keys = Vec::new();
    for index in 0..4 {
        let entry = (slab.try_grant_in(0..4)).expect("the first partition must admit four entries");
        let entry = entry.reserve();
        assert_eq!(entry.key().index(), index);
        let key = entry.key();
        entry.commit(index);
        keys.push(key);
    }
    assert!(matches!(
        slab.try_grant_in(0..4),
        Err(o3::depot::arena::slab::Vacancy::Full)
    ));

    let other = (slab.try_grant_in(4..8)).expect("the empty second partition must have a vacancy");
    let other = other.reserve();
    assert_eq!(other.key().index(), 4);
    let key = other.key();
    other.commit(4);
    let other = key;
    let combined =
        (slab.try_grant_in(0..8)).expect("the combined range must reach the second partition");
    assert_eq!(combined.reserve().key().index(), 5);

    let old = keys.swap_remove(2);
    assert_eq!(slab.occupied(old).map(|entry| entry.remove()), Some(2));
    let reused =
        (slab.try_grant_in(0..4)).expect("the released first-partition slot must be vacant");
    assert_eq!(reused.reserve().key().index(), 2);
    let entry = slab
        .occupied_at(other.index())
        .expect("occupied partition slot");
    assert!(entry.key() == other);
    assert_eq!(entry.remove(), 4);
}

#[test]
fn stale_after_wrap() {
    let mut slab = (Exclusive::<u32, Generation>::try_new(capacity::Count::new(1)))
        .expect("one wrapping slab slot must fit");

    let first = (slab.try_grant()).expect("the first logical identity must be available");
    let first = {
        let vacant = first.reserve();
        let key = vacant.key();
        vacant.commit(1);
        key
    };
    assert_eq!(slab.occupied(first).map(|entry| entry.remove()), Some(1));
    let second = (slab.try_grant()).expect("the second logical identity must be available");
    let second = {
        let vacant = second.reserve();
        let key = vacant.key();
        vacant.commit(2);
        key
    };
    assert_eq!(slab.occupied(second).map(|entry| entry.remove()), Some(2));
    let third = (slab.try_grant()).expect("the third logical identity must be available");
    let third = {
        let vacant = third.reserve();
        let key = vacant.key();
        vacant.commit(3);
        key
    };

    assert_eq!(first.generation().get(), 1);
    assert_eq!(second.generation().get(), 2);
    assert_eq!(third.generation().get(), 3);
    assert_eq!(first.index(), third.index());
    assert_eq!(slab.get(first), None);
    assert_eq!(slab.get(second), None);
    assert_eq!(slab.get(third), Some(&3));
    assert!(matches!(
        slab.try_grant(),
        Err(o3::depot::arena::slab::Vacancy::IdentityExhausted)
    ));
    assert_eq!(slab.occupied(third).map(|entry| entry.remove()), Some(3));
    assert!(matches!(
        slab.try_grant(),
        Err(o3::depot::arena::slab::Vacancy::IdentityExhausted)
    ));
}

#[test]
fn exclusive_cancel_epoch() {
    let mut slab = (Exclusive::<u32, Generation>::try_new(capacity::Count::new(2)))
        .expect("two exclusive slab slots must fit");
    {
        let free = (slab.try_grant_in(1..2)).expect("the indexed slot must initially be vacant");
        let free = free.reserve();
        assert_eq!(free.key().generation(), Generation::MIN);
    }

    let key = (slab.try_grant_in(1..2)).expect("the cancelled reservation must restore its slot");
    let key = {
        let vacant = key.reserve();
        let key = vacant.key();
        vacant.commit(7);
        key
    };
    assert_eq!(key.generation().get(), 2);
    assert!((slab.current(1)) == (Some((&7, key))));
    let value = (slab.get_mut(key)).expect("current key remains live");
    *value = 9;
    let entry = (slab.occupied(key)).expect("current entry remains live");
    assert_eq!(entry.get(), &9);
}

#[test]
fn shared_drop_spends_epoch() {
    let slab = (Storage::<u32, Generation>::try_new(capacity::Count::new(1)))
        .expect("one shared slab slot must fit");
    {
        let free = (slab.vacancy()).expect("the shared slot must initially be vacant");
        assert_eq!(free.key().generation(), Generation::MIN);
    }

    let key = (slab.vacancy()).expect("the cancelled shared vacancy must be restored");
    let key = key.insert(7);
    assert_eq!(key.generation().get(), 2);
    assert!(matches!(slab.try_entry_at(0).map(|entry| entry.key()), Ok(actual) if actual == key));
}

#[test]
fn shared_physical_key() {
    let slab = (Storage::<u32, Generation>::try_new(capacity::Count::new(2)))
        .expect("two shared slab slots must fit");
    let first = (slab.vacancy().map(|vacant| vacant.insert(1)))
        .expect("the first physical slot must be vacant");
    let second = (slab.vacancy().map(|vacant| vacant.insert(2)))
        .expect("the second physical slot must be vacant");
    assert_eq!((first.index(), second.index()), (0, 1));
    assert!(matches!(
        slab.try_entry(first).map(|entry| entry.take()),
        Ok(1)
    ));
    assert!(matches!(
        slab.try_entry_at(0),
        Err(lookup::Error::Free { index: 0 })
    ));
    assert!(
        matches!(slab.try_entry_at(1).map(|entry| entry.key()), Ok(actual) if actual == second)
    );
}

#[test]
fn shared_transactions() {
    let slab = (Storage::<u32, Generation>::try_new(capacity::Count::new(1)))
        .expect("one shared slot must allocate");
    let first = (slab.vacancy()).expect("the empty slab must reserve its first value");
    let first = first.insert(1);
    assert!(matches!(
        slab.vacancy(),
        Err(o3::depot::arena::slab::Vacancy::Full)
    ));
    assert!(matches!(
        slab.try_entry(first).map(|entry| entry.take()),
        Ok(1)
    ));
    let free = (slab.vacancy()).expect("the removed slot must accept a second generation");
    let key = free.key();
    let mut staged = free.stage(3);
    assert_eq!(key.generation().get(), 2);
    assert!(matches!(
        slab.try_entry(key),
        Err(Lookup::Physical {
            cause: lookup::Error::Busy { borrow: (), .. },
            ..
        })
    ));
    *staged.get_mut() = 4;
    staged.publish();
    assert!(matches!(slab.try_entry_at(0).map(|entry| entry.key()), Ok(actual) if actual == key));
    assert!(matches!(
        slab.try_entry(key).map(|entry| entry.take()),
        Ok(4)
    ));
    {
        let free = (slab.vacancy()).expect("the third generation must remain available");
        assert!(matches!(
            slab.vacancy(),
            Err(slab::Vacancy::IdentityExhausted)
        ));
        assert_eq!(free.key().generation().get(), 3);
        let mut staged = free.stage(6);
        *staged.get_mut() = 7;
    }
    assert!(slab.is_empty());
    assert!(matches!(
        slab.vacancy(),
        Err(o3::depot::arena::slab::Vacancy::IdentityExhausted)
    ));
}

#[test]
fn shared_reuse() {
    let slab = Storage::<u32, slab::key::Generation<6>>::try_new(Count::new(2))
        .expect("two shared slots must fit");
    let first = (slab.vacancy_at(0).map(|vacant| vacant.insert(1)))
        .expect("the empty shared slab must admit its first value");
    let entry = slab.try_entry(first);
    let entry = entry.expect("external key must retain its original logical generation");

    let value = entry.take();
    assert_eq!(value, 1);
    let vacant = slab.vacancy_at(first.index()).expect("the second identity");
    let stale = [3, 4].map(|epoch| {
        let key = slab
            .vacancy_at(1)
            .expect("the other slot remains available while zero is reserved")
            .insert(epoch);
        assert_eq!(key.generation().get(), epoch);
        assert_eq!(slab.try_entry(key).expect("issued neighbor").take(), epoch);
        key
    });
    let second = vacant.insert(2);
    assert_eq!(second.generation().get(), 2);
    let entry = slab.try_entry(second);
    let mut entry = entry.expect("external key must retain its original logical generation");

    *entry.get_mut() += 1;
    let value = entry.take();
    assert_eq!(value, 3);
    let vacant = slab
        .vacancy_at(second.index())
        .expect("identity after the neighbor");
    let third = vacant.insert(value);
    assert_eq!(third.generation().get(), 5);
    let current = slab
        .vacancy_at(1)
        .expect("the final identity reuses the other physical slot")
        .insert(6);
    assert_eq!(current.generation().get(), 6);
    for old in stale {
        assert!(matches!(
            slab.try_entry(old),
            Err(Lookup::Generation { key, actual })
                if key == old && actual == current.generation()
        ));
    }
    let entry = slab.try_entry(third);
    let entry = entry.expect("external key must retain its original logical generation");

    let value = entry.take();
    assert_eq!(value, 3);
    assert_eq!(slab.try_entry(current).expect("live neighbor").take(), 6);
    assert!(slab.is_empty());
    assert!(matches!(
        slab.vacancy(),
        Err(slab::Vacancy::IdentityExhausted)
    ));
}

#[test]
fn checked_lookup() {
    let mut slab =
        (Exclusive::<u32, Wide>::try_new(capacity::Count::new(1))).expect("one slot must allocate");
    let outside = external::Key::new(u32::MAX, Wide::MIN);
    assert!(matches!(slab.try_occupied(outside),
        Err(Lookup::Physical { key, cause: lookup::Error::Index { index: u32::MAX, capacity } })
        if key == outside && capacity == capacity::Count::new(1)));
    let vacant = external::Key::new(0, Wide::MIN);
    assert!(matches!(slab.try_occupied(vacant),
        Err(Lookup::Physical { key, cause: lookup::Error::Free { index: 0 } })
        if key == vacant));
    let grant = (slab.try_grant()).expect("the empty slot must admit a value");
    let old = {
        let vacant = grant.reserve();
        let key = vacant.key();
        vacant.commit(7);
        key
    };
    let entry = slab.try_occupied(old);
    let mut entry = entry.expect("external key must retain its original logical generation");

    *entry.get_mut() += 2;
    assert_eq!(entry.get(), &9);
    assert_eq!(entry.remove(), 9);
    let grant = (slab.try_grant()).expect("removal must restore the slot");
    let current = {
        let vacant = grant.reserve();
        let key = vacant.key();
        vacant.commit(11);
        key
    };
    assert!(matches!(slab.try_occupied(old),
        Err(Lookup::Generation { key, actual }) if key == old && actual == current.generation()));
    let entry = slab.try_occupied(current);
    let entry = entry.expect("external key must retain its original logical generation");

    assert_eq!(entry.remove(), 11);
    let grant = (slab.try_grant()).expect("the removed slot must admit a reservation");
    let reserved = grant.reserve();
    let pending = reserved.key();
    mem::forget(reserved);
    assert!(matches!(slab.try_occupied(pending),
        Err(Lookup::Physical { key, cause: lookup::Error::Reserved { index: 0 } })
        if key == pending));
}

#[test]
fn advances_past_retired() {
    let (mut slab,) = (({
        let actual: Result<slab::Store<u32, Short, 3>, _> =
            slab::Store::<u32, Short, 3>::try_new(Count::new(2));
        actual
    })
    .expect("the two-slot short-generation slab must allocate"),);
    let first = slab
        .vacant_at(0)
        .expect("slot zero begins vacant")
        .insert(1);
    assert_eq!(first.generation().get(), 1);
    assert_eq!(slab.remove(first), Some(1));
    let second = slab
        .vacant_at(0)
        .expect("removing the first value vacates slot zero")
        .insert(2);
    assert_eq!(second.generation().get(), 2);
    assert_eq!(slab.try_get(first).ok(), None);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab
        .vacant_at(0)
        .expect("removing the second value vacates slot zero")
        .insert(3);
    assert_eq!(third.generation().get(), 3);
    assert_eq!(slab.remove(third), Some(3));
    assert!(!slab.is_full());
    assert_eq!(
        slab.insert(4)
            .expect("retiring slot zero leaves slot one available")
            .index(),
        1
    );
    assert!(slab.is_full());

    let (slab,) = (({
        let actual: Result<slab::Cell<u32, Short, 3, false>, _> =
            slab::Cell::<u32, Short, 3, false>::try_new(Count::new(2));
        actual
    })
    .expect("the two-slot short-generation cell slab must allocate"),);
    let first = slab
        .vacancy()
        .map(|vacant| vacant.insert(1))
        .expect("the fresh cell slab has a slot");
    assert_eq!(
        slab.entry(first.parts()).map(|entry| {
            let value = *entry.value();
            entry.discard();
            value
        }),
        Some(1)
    );
    let second = slab
        .vacancy()
        .map(|vacant| vacant.insert(2))
        .expect("removing the first cell value recycles its slot");
    assert_eq!(second.generation().get(), 2);
    assert_eq!(
        slab.entry(second.parts()).map(|entry| {
            let value = *entry.value();
            entry.discard();
            value
        }),
        Some(2)
    );
    let third = slab
        .vacancy()
        .map(|vacant| vacant.insert(3))
        .expect("removing the second cell value recycles its slot");
    assert_eq!(third.generation().get(), 3);
    assert_eq!(
        slab.entry(third.parts()).map(|entry| {
            let value = *entry.value();
            entry.discard();
            value
        }),
        Some(3)
    );
    assert!(!slab.is_full());
    assert_eq!(slab.update(first, |_| ()), None);
    assert_eq!(slab.update(second, |_| ()), None);
    assert_eq!(
        slab.vacancy()
            .map(|vacant| vacant.insert(4))
            .expect("retiring cell slot zero leaves slot one available")
            .index(),
        1
    );
    assert!(slab.is_full());
}

#[test]
fn cancel_keeps_generation() {
    let (mut slab,) = (({
        let actual: Result<slab::Store<u32, Short, 3>, _> =
            slab::Store::<u32, Short, 3>::try_new(Count::new(1));
        actual
    })
    .expect("the short-generation cancellation slab must allocate"),);
    assert!(slab.vacant_at(0).is_some());
    let second = slab
        .vacant_at(0)
        .expect("canceling vacancy keeps slot zero available")
        .insert(2);
    assert_eq!(second.generation().get(), 1);
    assert_eq!(slab.remove(second), Some(2));
    let third = slab
        .vacant_at(0)
        .expect("removing the second value vacates slot zero")
        .insert(3);
    assert_eq!(third.generation().get(), 2);
    assert_eq!(slab.remove(third), Some(3));
    let fourth = slab
        .insert(4)
        .expect("the final live generation remains available");
    assert_eq!(fourth.generation().get(), 3);
    assert_eq!(slab.remove(fourth), Some(4));
    assert!(slab.insert(5).is_err());
    assert!(slab.is_full());

    let (mut slab,) = (({
        let actual: Result<slab::Store<u32>, _> = slab::Store::<_>::try_new(Count::new(1));
        actual
    })
    .expect("the full-generation cancellation slab must allocate"),);
    assert!(slab.try_vacant().is_ok());
    let fresh = slab
        .insert(7)
        .expect("canceling the vacancy leaves its generation unchanged");
    assert_eq!(fresh.generation().get(), 1);
    assert_eq!(slab.try_get(fresh).ok(), Some(&7));
}

#[test]
fn projects_logical_neighbors() {
    let mut slots =
        Exclusive::<_, o3::base::num::Epoch>::try_new(Count::new(2)).expect("two slots");
    let [first, second] = [11, 22].map(|value| {
        let entry = slots
            .try_grant()
            .expect("logical identity")
            .reserve()
            .stage_with(|index| (index, value))
            .commit();
        (entry.key(), entry.get().0)
    });
    let mut context = 0;
    // SAFETY: both entries remain initialized in this exact exclusively borrowed owner.
    let live =
        unsafe { Proof::<_, Initialized<Short>>::new(((&mut slots, &mut context), first.1)) };
    let (entry, context) = external::OccupiedSlot::from_live(live);
    *context = 7;
    // SAFETY: the retained root still owns the other initialized slot.
    let neighbor = unsafe { Proof::<_, Initialized<()>>::new((&entry, second.1)) };
    let (key, value) = external::OccupiedSlot::project(neighbor);
    assert_eq!((entry.key(), key, value.1), (first.0, second.0, 22));
    assert_eq!((*context, entry.remove().1), (7, 11));
    assert_eq!(slots.get(second.0).map(|value| value.1), Some(22));
}
