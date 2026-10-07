pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::collections::VecDeque;

use common::{
    alloc::system::Tracker,
    depot::{
        model::Random,
        queue::indexed::keyed::{Entry, Error, Lanes},
    },
};
use o3::depot::{
    arena::slab::key,
    capacity,
    queue::indexed::keyed::{self, lanes},
};

#[test]
fn retains_validated_state() {
    let mut owner = (Lanes::try_new(4)).expect("four fixture slots must allocate");
    let keys: Vec<_> = owner.as_slice().iter().map(|slot| slot.key).collect();
    for key in &keys[..3] {
        let vacant = (owner.vacant(*key)).expect("keyed lane must retain its exact membership");

        vacant.push_back::<0>();
    }
    let vacant = (owner.vacant(keys[3])).expect("keyed lane must retain its exact membership");

    vacant.push_back::<1>();
    let epoch = (key::Generation::MIN.checked_add(1)).expect("next generation");
    assert!(
        matches!(owner.try_replace(keys[1], epoch, 0), Err(Error::Linked(key)) if key == keys[1])
    );
    let checks = owner.checks.expect("validation count");
    let mut member = (owner.member(keys[1])).expect("keyed lane must retain its exact membership");

    assert_eq!(*member.get(), 1);
    member.move_to_back::<1>();
    member.move_to_back::<1>();
    let mut vacant = member.detach();
    *vacant.edit() = 19;
    let mut member = vacant.push_back::<0>();
    assert_eq!(*member.get(), 19);
    member.move_to_back::<1>();
    assert_eq!(owner.checks, Some(checks + 1));
    let member = (owner.member(keys[2])).expect("keyed lane must retain its exact membership");

    assert!((member.detach().take()) == (keys[2]));
    assert!(matches!(owner.member(keys[2]), Err(Error::Detached(key)) if key == keys[2]));
    assert!(
        (lanes::Bound::front::<0, _>(&mut owner).map(|entry| entry.detach().take()))
            == (Some(keys[0]))
    );
    assert!(lanes::Bound::front::<0, _>(&mut owner).is_none());
    for key in [keys[3], keys[1]] {
        assert!(
            (lanes::Bound::front::<1, _>(&mut owner).map(|entry| entry.detach().take()))
                == (Some(key))
        );
    }
    assert!(owner.as_ref().is_empty::<1>());
    let fresh = owner
        .try_replace(keys[3], epoch, 0)
        .expect("detached replacement");
    assert!(
        matches!(owner.vacant(keys[3]), Err(Error::Stale { requested, current }) if requested == keys[3] && current == fresh)
    );
    let vacant = (owner.vacant(fresh)).expect("keyed lane must retain its exact membership");

    vacant.push_back::<0>();
    let outside = key::Handle::from_parts(key::Parts::new(4, epoch));
    assert!(
        matches!(owner.vacant(outside), Err(Error::Bounds { requested, capacity: 4 }) if requested == outside)
    );
}

#[test]
fn matches_endpoint_model() {
    let mut owner = (Lanes::try_new(8)).expect("eight model slots must allocate");
    let keys: Vec<_> = owner.as_slice().iter().map(|slot| slot.key).collect();
    let mut model = [VecDeque::new(), VecDeque::new()];
    let mut rng = Random::<u64, 13, 7, 17>::try_new(0x6a09e667f3bcc909u64).expect("shifts");
    for _ in 0..20_000 {
        let seed = rng.advance();
        let index = seed as usize % keys.len();
        let target = (seed >> 9) as usize % 2;
        let key = keys[index];
        let linked = model.iter().position(|lane| lane.contains(&key));
        match (linked, seed >> 17 & 3) {
            (Some(source), 0) => {
                let member =
                    (owner.member(key)).expect("keyed lane must retain its exact membership");

                member.detach().take();
                model[source].retain(|item| *item != key);
            }
            (Some(source), _) => {
                let mut member =
                    (owner.member(key)).expect("keyed lane must retain its exact membership");

                if target == 0 {
                    member.move_to_back::<0>();
                } else {
                    member.move_to_back::<1>();
                }
                model[source].retain(|item| *item != key);
                model[target].push_back(key);
            }
            (None, _) => {
                let vacant =
                    (owner.vacant(key)).expect("keyed lane must retain its exact membership");

                if target == 0 {
                    vacant.push_back::<0>();
                } else {
                    vacant.push_back::<1>();
                }
                model[target].push_back(key);
            }
        }
        assert!(
            (lanes::Bound::front::<0, _>(&mut owner).map(|entry| entry.key()))
                == (model[0].front().copied())
        );
        assert!(
            (lanes::Bound::front::<1, _>(&mut owner).map(|entry| entry.key()))
                == (model[1].front().copied())
        );
    }
    while let Some(entry) = lanes::Bound::front::<0, _>(&mut owner) {
        assert!((Some(entry.detach().take())) == (model[0].pop_front()));
    }
    while let Some(entry) = lanes::Bound::front::<1, _>(&mut owner) {
        assert!((Some(entry.detach().take())) == (model[1].pop_front()));
    }
    assert!(model.iter().all(VecDeque::is_empty));
}

#[test]
fn rejects_overlap() {
    assert!(lanes::Fifo::<key::Handle, 2>::try_new(capacity::Count::new(u32::MAX)).is_err());
    lanes::Fifo::<key::Handle, 2>::try_new(capacity::Count::new(u32::MAX - 1))
        .as_ref()
        .expect("required fixture outcome");
    lanes::Fifo::<key::Handle, 2>::try_new(capacity::Count::new(0))
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(std::mem::size_of::<lanes::Fifo<key::Handle, 2>>(), 16);
    assert_eq!(std::mem::size_of::<keyed::Link<key::Handle>>(), 8);
    assert_eq!(
        std::mem::size_of::<lanes::Bound<Entry<'_>, 2>>(),
        std::mem::size_of::<Entry<'_>>()
    );
    assert_eq!(
        std::mem::size_of::<lanes::Bound<Entry<'_>, 2, (), false>>(),
        std::mem::size_of::<Entry<'_>>()
    );
}
