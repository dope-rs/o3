pub mod common;

use std::array;

use common::{alloc::system::Tracker, depot::model::Random};
use o3::depot::{
    array::Index,
    queue::group::{self, Ledger, capacity, drain, vacant},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn cancels_reservation() {
    let capacity = capacity::Count::try_from(3).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 1>::try_new(capacity).expect("ledger allocation");
    assert!(
        (ledger.push_back(Index::at::<0>(), 7, 70)).is_ok(),
        "required fixture outcome"
    );
    let ((), counts) = Tracker::measure(|| {
        let error = group::Batch::<_, 1, 3>::new(&mut ledger)
            .err()
            .expect("the batch must reject insufficient capacity");
        assert!(matches!(error, group::Error::Entries(shortage)
            if shortage.requested.get() == 3 && shortage.available == 2));
        assert!(
            group::Batch::<_, 1, 2>::new(&mut ledger).is_ok(),
            "queue group must fit its declared capacity"
        );
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(ledger.available(), 2);
    assert!(ledger.pop_front(Index::at::<0>()) == Some((7, 70)));
}

#[test]
fn models_root_changes() {
    use std::collections::VecDeque;

    let capacity = capacity::Count::try_from(32).expect("tagged capacity");
    let mut ledger = Ledger::<u64, 2>::try_new(capacity).expect("ledger allocation");
    let mut model: [VecDeque<(u64, u64)>; 2] = array::from_fn(|_| VecDeque::with_capacity(32));
    let mut rng = Random::<u64, 13, 7, 17>::try_new(0x7a31_e029_d401_a5bdu64).expect("shifts");
    for step in 0..20_000 {
        let seed = rng.advance();
        let index = (seed as usize >> 8) & 1;
        let lane = [Index::at::<0>(), Index::at::<1>()][index];
        let owner = match seed % 65 {
            0 => 0,
            bit => 1 << (bit - 1),
        };
        let free = ledger.available();
        match seed % 7 {
            0 if free >= 3 => {
                let items = [owner, owner ^ u64::MAX, owner]
                    .map(|owner| vacant::Item::new(lane, owner, step));
                let (result, counts) = Tracker::measure(|| {
                    let batch = group::Batch::new(&mut ledger);
                    let batch = batch.expect("the prepared batch must be available");
                    batch.publish(items, (), |()| {}, |value| value)
                });
                assert!((result).is_ok(), "required fixture outcome");
                assert_eq!(counts, [0; 4]);
                model[index].extend([owner, owner ^ u64::MAX, owner].map(|key| (key, step)));
            }
            1 if free != 0 => {
                let (result, counts) = Tracker::measure(|| ledger.push_back(lane, owner, step));
                assert!((result).is_ok(), "required fixture outcome");
                assert_eq!(counts, [0; 4]);
                model[index].push_back((owner, step));
            }
            2 => {
                let (taken, counts) = Tracker::measure(|| ledger.pop_front(lane));
                assert_eq!(taken, model[index].pop_front());
                assert_eq!(counts, [0; 4]);
            }
            3 => {
                let expected = model[index].front().copied();
                let ((), counts) = Tracker::measure(|| {
                    if let Some(taken) = ledger.bind_front(lane).map(group::Front::take) {
                        let (value, slot) = taken;
                        assert_eq!(Some(value), expected.map(|entry| entry.1));
                        if step & 1 == 0 {
                            slot.restore(value);
                        }
                    }
                });
                if step & 1 != 0 {
                    model[index].pop_front();
                }
                assert_eq!(counts, [0; 4]);
            }
            4 => {
                let before = model[index].len();
                let mut expected = model[index]
                    .iter()
                    .filter(|entry| entry.0 == owner)
                    .map(|entry| entry.1);
                let (removed, counts) = Tracker::measure(|| {
                    ledger.drain_owner(owner, lane, |value| {
                        assert_eq!(Some(value), expected.next())
                    })
                });
                assert_eq!(expected.next(), None);
                model[index].retain(|entry| entry.0 != owner);
                assert_eq!(removed, before - model[index].len());
                assert_eq!(counts, [0; 4]);
            }
            _ if free != 0 => {
                let ((), counts) = Tracker::measure(|| {
                    assert!(
                        group::Batch::<_, 2, 1>::new(&mut ledger)
                            .and_then(|batch| batch.bind(lane, owner))
                            .is_ok(),
                        "queue group must fit its declared capacity"
                    );
                });
                assert_eq!(counts, [0; 4]);
            }
            _ => {}
        }
        for (index, lane) in [Index::at::<0>(), Index::at::<1>()].into_iter().enumerate() {
            assert_eq!(ledger.lane_len(lane), model[index].len());
            assert_eq!(
                ledger.front(lane).copied(),
                model[index].front().map(|entry| entry.1)
            );
        }
        assert_eq!(
            ledger.available(),
            32 - model.iter().map(VecDeque::len).sum::<usize>()
        );
    }
}

#[test]
fn retains_partial_drain() {
    let capacity = capacity::Count::try_from(4).expect("tagged capacity");
    let mut ledger = Ledger::<usize, 2>::try_new(capacity).expect("ledger allocation");
    for (owner, value) in [(7, 10), (8, 20), (7, 11)] {
        assert!(
            (ledger.push_back(Index::at::<0>(), owner, value)).is_ok(),
            "required fixture outcome"
        );
    }
    assert!(
        (ledger.push_back(Index::at::<1>(), 7, 99)).is_ok(),
        "required fixture outcome"
    );
    let ((), counts) = Tracker::measure(|| {
        let mut drain = drain::Drain::new(&mut ledger, 7, Index::at::<0>())
            .expect("the existing owner must bind once");
        let item = drain
            .next_entry()
            .expect("the owner's first record must resolve");
        assert_eq!(*item.get(), 10);
        assert_eq!(item.remove(), 10);
        let item = drain
            .next_entry()
            .expect("the successor must retain its owner");
        assert_eq!(*item.get(), 11);
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(ledger.available(), 1);
    assert!(ledger.get(7, Index::at::<0>()) == Some(&11));
    assert!(ledger.get(7, Index::at::<1>()) == Some(&99));
    assert!(ledger.drain_owner(7, Index::at::<0>(), |value| assert_eq!(value, 11)) == 1);
    assert!(ledger.pop_front(Index::at::<0>()) == Some((8, 20)));
    assert!(ledger.pop_front(Index::at::<1>()) == Some((7, 99)));
    assert_eq!(ledger.available(), 4);
}
