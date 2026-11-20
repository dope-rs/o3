pub mod common;

use std::{cell, collections::VecDeque, num::NonZero};

use common::{
    Counted,
    depot::model::{CAPACITY, Random, STEPS},
};
use o3::depot::{
    arena::slab::{self, key},
    capacity,
    queue::round::{self, set},
};

#[test]
fn consumes_membership() {
    capacity::Domain::with(capacity::Count::new(3), |domain| {
        let mut owner = round::Set::<u32>::try_in(domain).expect("allocate residents");
        for (value, entry) in owner.entries().enumerate() {
            let entry = match entry {
                set::Entry::Vacant(entry) => Some(entry),
                _ => None,
            }
            .expect("fresh resident is vacant");
            let ready = entry.insert(value as u32).insert();
            let idle = ready.remove();
            assert_eq!(*idle.get(), value as u32);
            idle.insert();
        }
        assert!((owner.len()) == (capacity::Count::new(3)));
        let first = (owner.turn()).expect("first turn");
        assert_eq!(*first.advance(), 0);
        let second = (owner.turn()).expect("second turn");
        assert_eq!(*second.get(), 1);
        second.remove().discard();
        let third = (owner.turn()).expect("third turn");
        assert_eq!(*third.advance(), 2);
        let first = (owner.turn()).expect("wrapped turn");
        assert_eq!(*first.get(), 0);
        first.remove().discard();
        let last = (owner.turn()).expect("singleton turn");
        assert_eq!(*last.get(), 2);
        last.remove().discard();
        assert!(owner.turn().is_none());
        assert!(owner.is_empty());
    });
}
#[test]
fn matches_model() {
    const CAPACITY: u32 = 31;
    capacity::Domain::with(capacity::Count::new(CAPACITY), |domain| {
        let mut owner = round::Set::<u32>::try_in(domain).expect("allocate model residents");
        let keys: Vec<_> = owner
            .entries()
            .enumerate()
            .map(|(index, entry)| {
                let entry = match entry {
                    set::Entry::Vacant(entry) => Some(entry),
                    _ => None,
                }
                .expect("fresh physical slot");
                entry.insert(index as u32).key()
            })
            .collect();
        let mut model = VecDeque::new();
        let mut rng = Random::<u64, 7, 9, 8>::try_new(0xa076_1d64_78bd_642f_u64).expect("shifts");
        for _ in 0..100_000 {
            let random = rng.advance();
            let index = (random % u64::from(CAPACITY)) as usize;
            let key = keys[index];
            match (random >> 16) % 5 {
                0 | 1 => {
                    let entry = (owner.try_entry(key))
                        .expect("slab key must retain its original slot observation");
                    match entry.state() {
                        set::State::Idle(entry) => {
                            assert!(!model.contains(&index));
                            entry.insert();
                            model.push_back(index);
                        }
                        set::State::Ready(_) => assert!(model.contains(&index)),
                    }
                }
                2 | 3 => {
                    let entry = (owner.try_entry(key))
                        .expect("slab key must retain its original slot observation");
                    match entry.state() {
                        set::State::Idle(_) => assert!(!model.contains(&index)),
                        set::State::Ready(entry) => {
                            let position = (model.iter().position(|value| *value == index))
                                .expect("linked model resident");
                            assert_eq!(model.remove(position), Some(index));
                            if (random >> 16) % 5 == 3 {
                                entry.requeue();
                                model.push_back(index);
                            } else {
                                entry.remove();
                            }
                        }
                    }
                }
                _ => {
                    let expected = model.pop_front();
                    if let Some(index) = expected {
                        model.push_back(index);
                    }
                    assert_eq!(owner.turn().map(|head| *head.advance() as usize), expected);
                }
            }
        }
    });
}

#[test]
fn reuse_preserves_membership() {
    capacity::Domain::with(capacity::Count::new(1), |domain| {
        let old_drops = cell::Cell::new(Some(0));
        let new_drops = cell::Cell::new(Some(0));
        let mut owner = round::Set::<_, (), 2>::try_in(domain).expect("one resident");
        let old = owner
            .try_vacant()
            .expect("first slot")
            .insert(Counted(&old_drops))
            .insert()
            .key();
        assert!(matches!(owner.try_vacant(), Err(slab::Vacancy::Full)));

        let entry = owner.try_entry(old).expect("first generation");
        let admission = set::Reuse::try_from(entry)
            .map_err(|entry| *entry.key().as_inner())
            .expect("successor");
        std::mem::forget(admission);
        let entry = owner
            .try_entry(old)
            .expect("abandoned admission preserves identity");
        let reuse = set::Reuse::try_from(entry)
            .map_err(|entry| *entry.key().as_inner())
            .expect("successor");
        assert_eq!(reuse.key(), old);
        let (last, prior) = reuse.replace(Counted(&new_drops));
        let key = last.key();
        assert_eq!(key.as_inner().index(), old.as_inner().index());
        assert_eq!(key.as_inner().generation().get(), 2);
        assert_eq!(old_drops.get(), Some(0));
        drop(prior);
        let retained = set::Reuse::try_from(last)
            .err()
            .expect("terminal generation returns its owner");
        assert_eq!(retained.key(), key);
        drop(retained);
        assert!(
            matches!(owner.try_entry(old), Err(key::Stale::Generation { key: rejected, actual })
            if rejected == old && actual == key.as_inner().generation())
        );
        let head = owner.turn().expect("replacement stays scheduled");
        assert_eq!(head.key(), key);
        head.remove().discard();
        assert!(matches!(
            owner.try_vacant(),
            Err(slab::Vacancy::IdentityExhausted)
        ));
        assert!(
            matches!(owner.try_entry(key), Err(key::Stale::Retired { key: rejected, actual })
            if rejected == key && actual == key.as_inner().generation())
        );
        assert_eq!((old_drops.get(), new_drops.get()), (Some(1), Some(1)));
    });
}

#[test]
fn keeps_disjoint_members() {
    capacity::Domain::with(capacity::Count::new(3), |domain| {
        let mut owner = round::Set::<u32>::try_in(domain).expect("joint owner");
        {
            let mut entries = owner.entries().enumerate().map(|(index, entry)| {
                let entry = match entry {
                    set::Entry::Vacant(entry) => Some(entry),
                    _ => None,
                }
                .expect("fresh resident");
                entry.insert(index as u32 + 1)
            });
            let first = entries.next().expect("first resident").insert();
            let second = entries.next().expect("second resident").insert();
            let third = entries.next().expect("third resident");
            first.remove().discard();
            let third = third.insert();
            second.requeue();
            *third.remove().insert().into_mut() = 30;
        }
        owner.try_vacant().expect("released first slot").insert(1);
        assert!(owner.values().copied().eq([1, 2, 30]));
        assert_eq!(owner.turn().map(|head| *head.advance()), Some(2));
        assert_eq!(owner.turn().map(|head| *head.advance()), Some(30));
    });
}

#[test]
fn matches_striped_lanes() {
    const LANES: usize = 11;
    const GROUPS: usize = 3;

    let mut queue = (round::Lanes::<cell::Cell<usize>, GROUPS>::try_new(
        CAPACITY,
        NonZero::<usize>::MIN.saturating_add(LANES - 1),
    ))
    .expect("the striped lane model must allocate");
    for index in 0..CAPACITY {
        let entry = queue.try_slot(index).expect("resident").get();
        assert_eq!(entry.get(), 0);
        entry.set(index);
    }
    let mut model = vec![VecDeque::new(); LANES];
    let mut active = vec![VecDeque::new(); GROUPS];
    let mut used = [None; CAPACITY];
    let mut random = Random::<u64, 7, 9, 8>::try_new(0x94d0_49bb_1331_11eb).expect("shifts");

    for _ in 0..STEPS {
        let bits = random.advance();
        let index = bits as usize % CAPACITY;
        let track = (bits >> 8) as usize % LANES;
        let stripe = track % GROUPS;
        let lane = round::Lane::try_new(&queue, track).expect("lane");
        let groups = [queue.group::<0>(), queue.group::<1>(), queue.group::<2>()];
        let group = &groups[stripe];

        match (bits >> 16) % 6 {
            0 | 1 if used[index].is_none() => {
                let wake = model[track].is_empty();
                assert!(
                    queue
                        .try_slot(index)
                        .expect("resident")
                        .try_lane(track)
                        .expect("destination")
                        .set()
                );
                model[track].push_back(index);
                if wake {
                    active[stripe].push_back(track);
                }
                used[index] = Some(track);
            }
            2 => {
                let actual = lane.pop_front().map(|slot| slot.get());
                let want = model[track].pop_front();
                assert_eq!(actual, want);
                if let Some(index) = want {
                    used[index] = None;
                    if model[track].is_empty() {
                        let pos = (active[stripe].iter().position(|item| *item == track))
                            .expect("an active striped lane is in its group model");
                        active[stripe].remove(pos);
                    }
                }
            }
            3 => {
                if let Some(owner) = used[index] {
                    let member = owner % GROUPS;
                    let gone = queue.try_slot(index).expect("resident").clear().get();
                    let pos = (model[owner].iter().position(|item| *item == index))
                        .expect("an occupied striped entry is in its lane model");
                    assert_eq!(model[owner].remove(pos), Some(gone));
                    used[index] = None;
                    if model[owner].is_empty() {
                        let pos = (active[member].iter().position(|item| *item == owner))
                            .expect("an active occupied lane is in its group model");
                        active[member].remove(pos);
                    }
                }
            }
            4 if !model[track].is_empty() => {
                let want =
                    (model[track].pop_front()).expect("a nonempty striped lane model has a front");
                let mut selected = round::Lane::try_new(&mut queue, track).expect("lane");
                let held = (selected.dequeue()).expect("a nonempty striped lane must dequeue");
                assert_eq!(held.get().get(), want);
                used[want] = None;
                let pos = (active[stripe].iter().position(|item| *item == track))
                    .expect("the selected active lane is in its group model");
                active[stripe].remove(pos);
                if !model[track].is_empty() {
                    active[stripe].push_back(track);
                }
            }
            5 if !active[stripe].is_empty() => {
                let next = (active[stripe].pop_front())
                    .expect("a nonempty striped group model has an active lane");
                let want =
                    (model[next].pop_front()).expect("an active striped lane model has a front");
                let actual = group.next().map(|(lane, slot)| (lane, slot.get()));
                assert_eq!(actual, Some((next, want)));
                used[want] = None;
                if !model[next].is_empty() {
                    active[stripe].push_back(next);
                }
            }
            _ => {}
        }

        for (lane, values) in model.iter().enumerate() {
            let lane = round::Lane::try_new(&queue, lane).expect("lane");
            assert_eq!(
                lane.front().map(|front| front.index()),
                values.front().copied()
            );
        }
        for (group, lanes) in active.iter().enumerate() {
            let groups = [queue.group::<0>(), queue.group::<1>(), queue.group::<2>()];
            let group = &groups[group];
            assert_eq!(
                group.active().map(|turn| turn.lane()),
                lanes.front().copied()
            );
        }
        for (index, lane) in used.iter().enumerate() {
            assert_eq!(
                queue.try_slot(index).expect("resident").is_linked(),
                lane.is_some()
            );
            assert_eq!(queue.try_slot(index).expect("resident").get().get(), index);
        }
    }
}
