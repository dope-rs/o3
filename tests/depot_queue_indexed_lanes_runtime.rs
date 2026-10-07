pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::collections::VecDeque;

use common::{
    alloc::system::Tracker,
    depot::model::{CAPACITY, Random, STEPS},
};
use o3::{
    base::cell::region,
    depot::{
        capacity,
        queue::{
            indexed::lanes::{
                entry,
                runtime::{self, shared},
            },
            tagged::{self, entry::Error},
        },
    },
};

#[test]
fn orders_runtime_lanes() {
    region::Token::scope(|mut token| {
        let queue =
            shared::Fifo::<u64>::try_from(capacity::Count::new(4), capacity::Count::new(3), &token);
        let queue = queue.expect("required fixture outcome");
        queue.head(2).as_ref().expect("required fixture outcome");
        for (lane, index, value) in [(0, 1, 11), (2, 3, 13), (2, 0, 10)] {
            let head = queue.head(lane);
            let head = head.expect("required fixture outcome");
            let mut lane = head.borrow(&mut token);
            let vacant = match lane.entry(index) {
                Ok(entry::State::Vacant(vacant)) => Some(vacant),
                _ => None,
            }
            .expect("the fixture slot must be vacant");
            vacant.insert::<false>(value);
        }
        let head = queue.head(2);
        let head = head.expect("required fixture outcome");
        let mut lane = head.borrow(&mut token);
        assert!(matches!(lane.entry(1), Ok(entry::State::Occupied(&11))));
        assert!(matches!(lane.entry(3), Ok(entry::State::Occupied(&13))));
        assert!(matches!(
            lane.vacant(1),
            Err(Error::Linked {
                index,
                previous: u32::MAX,
                next: u32::MAX,
            }) if index.get() == 1
        ));
        assert_eq!(lane.pop_entry(), Some((3, 13)));
        assert!(matches!(
            lane.entry(4),
            Err(entry::Error { index: 4, capacity: actual })
                if actual == capacity::Count::new(4)
        ));
        assert!(matches!(
            lane.vacant(4),
            Err(Error::Index {
                index: 4,
                capacity: actual
            }) if actual == capacity::Count::new(4)
        ));
        assert!(matches!(
            queue.head(3),
            Err(tagged::lane::Error { lane: 3, lanes: actual })
                if actual == capacity::Count::new(3)
        ));
        assert!(
            (queue
                .head(0)
                .map(|head| head.borrow(&mut token).pop_front()))
                == (Ok(Some(11)))
        );
        assert!(
            (queue
                .head(2)
                .map(|head| head.borrow(&mut token).pop_front()))
                == (Ok(Some(10)))
        );
        assert!(
            (queue
                .head(2)
                .map(|head| head.borrow(&mut token).pop_front()))
                == (Ok(None))
        );
    });
}

#[test]
fn borrows_runtime_values() {
    region::Token::scope(|mut token| {
        let text = String::from("borrowed");
        let queue =
            shared::Fifo::try_from(capacity::Count::new(1), capacity::Count::new(2), &token);
        let queue = queue.expect("required fixture outcome");
        let head = queue.head(1);
        let head = head.expect("required fixture outcome");
        let mut lane = head.borrow(&mut token);
        let vacant = lane.vacant(0);
        let vacant = vacant.expect("required fixture outcome");
        vacant.insert::<false>(text.as_str());
        assert_eq!(lane.pop_front(), Some(text.as_str()));
        assert_eq!(
            std::mem::size_of_val(&queue),
            std::mem::size_of::<[usize; 4]>()
        );
    });
}

#[test]
fn matches_runtime_lanes() {
    region::Token::scope(|mut token| {
        const LANES: usize = 7;
        let queue = shared::Fifo::<u64>::try_from(
            capacity::Count::new(CAPACITY as u32),
            capacity::Count::new(LANES as u32),
            &token,
        );
        let queue = queue.expect("required fixture outcome");
        let mut model = vec![VecDeque::new(); LANES];
        let mut used = [None; CAPACITY];
        let mut gens = [0_u32; CAPACITY];
        let mut random = Random::<u64, 7, 9, 8>::try_new(0xd1b5_4a32_d192_ed03).expect("shifts");

        for _ in 0..STEPS {
            let bits = random.advance();
            let index = bits as usize % CAPACITY;
            let lane = (bits >> 8) as usize % LANES;
            match (bits >> 16) % 5 {
                0 | 1 if used[index].is_none() => {
                    gens[index] = gens[index].wrapping_add(1);
                    let value = ((gens[index] as u64) << 32) | index as u64;
                    let head = queue.head(lane);
                    let head = head.expect("required fixture outcome");
                    let mut guard = head.borrow(&mut token);
                    let vacant = guard.vacant(index);
                    let vacant = vacant.expect("required fixture outcome");
                    vacant.insert::<false>(value);
                    model[lane].push_back(value);
                    used[index] = Some(lane);
                }
                2 => {
                    let actual = queue
                        .head(lane)
                        .map(|head| head.borrow(&mut token).pop_front());
                    let want = model[lane].pop_front();
                    assert!((actual) == (Ok(want)));
                    if let Some(value) = want {
                        used[value as u32 as usize] = None;
                    }
                }
                3 => {
                    if let Some(owner) = used[index] {
                        let value = ((gens[index] as u64) << 32) | index as u64;
                        assert!(
                            (queue.head(0).map(|head| head.take(
                                &mut token,
                                index,
                                value ^ (1 << 32)
                            ))) == (Ok(None))
                        );
                        assert!(
                            (queue
                                .head(0)
                                .map(|head| head.take(&mut token, index, value)))
                                == (Ok(Some(value)))
                        );
                        let at = model[owner].iter().position(|item| *item == value);
                        let at = at.expect("assertion failed: at.is_some()");
                        assert_eq!(model[owner].remove(at), Some(value));
                        used[index] = None;
                    }
                }
                4 if used[index].is_some() => {
                    let owner = used[index];
                    let owner = owner.expect("assertion failed: owner.is_some()");
                    let at = model[owner]
                        .iter()
                        .position(|value| *value as u32 as usize == index);
                    let at = at.expect("assertion failed: at.is_some()");
                    let endpoint = u32::MAX - owner as u32;
                    let previous = if at == 0 {
                        endpoint
                    } else {
                        model[owner][at - 1] as u32
                    };
                    let next = model[owner]
                        .get(at + 1)
                        .map_or(endpoint, |value| *value as u32);
                    let head = queue.head(lane);
                    let head = head.expect("required fixture outcome");
                    let mut guard = head.borrow(&mut token);
                    assert!(matches!(guard.vacant(index),
                        Err(Error::Linked { index: actual, previous: before, next: after })
                        if actual.get() == index && before == previous && after == next
                    ));
                }
                _ => {}
            }
        }
        for (lane, values) in model.iter_mut().enumerate() {
            let head = queue.head(lane);
            let head = head.expect("required fixture outcome");
            let mut guard = head.borrow(&mut token);
            while let Some(want) = values.pop_front() {
                assert_eq!(guard.pop_front(), Some(want));
            }
            assert_eq!(guard.pop_front(), None);
        }
    });
}

#[test]
fn reuses_runtime_heads() {
    use o3::base::cell::region;
    use runtime::shared;

    region::Token::scope(|mut token| {
        let (built, cold) = Tracker::measure(|| {
            shared::Fifo::<usize>::try_from(
                capacity::Count::new(3),
                capacity::Count::new(2),
                &token,
            )
        });
        let queue = (built).expect("tagged queue domains must be disjoint");

        assert_eq!(cold[0], 1);
        let head = (queue.head(1)).expect("queue lane must be in its original domain");

        let (result, hot) = Tracker::measure(|| {
            {
                let mut guard = head.borrow(&mut token);
                let vacant = guard.vacant(2)?;
                vacant.insert::<false>(19);
            }
            Ok::<_, Error>(head.borrow(&mut token).pop_front())
        });
        assert!((result) == (Ok(Some(19))));
        assert_eq!(hot, [0; 4]);
    });
}
