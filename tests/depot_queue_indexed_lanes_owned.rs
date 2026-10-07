use o3::depot::array::Index;
pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{
    cell::{self, Cell},
    collections::VecDeque,
};

use common::{
    Counted, Owner,
    alloc::system::Tracker,
    depot::{
        index::{capacity::Count, key::Id},
        model::{Random, drops},
        queue::indexed::Affine,
    },
};
use o3::depot::{capacity, queue::indexed::lanes};

#[test]
fn rejects_large_capacity() {
    assert!(
        lanes::runtime::Fifo::<u8>::try_from(capacity::Count::MAX, capacity::Count::new(2))
            .is_err()
    );
}

#[test]
fn high_lane_is_exact() {
    let mut queue = (lanes::Fifo::<u8, 17, _>::try_from(
        Count::<3>::try_new(capacity::Count::new(3)).expect("count fits bound"),
    ))
    .expect("the high-lane FIFO must allocate");
    queue
        .vacant::<16>(0)
        .map(|vacant| vacant.insert::<false>(10).commit())
        .as_ref()
        .expect("the first high-lane slot must be vacant");
    queue
        .vacant::<16>(1)
        .map(|vacant| vacant.insert::<false>(11).commit())
        .as_ref()
        .expect("the second high-lane slot must be vacant");
    queue
        .vacant::<16>(2)
        .map(|vacant| vacant.insert::<false>(12).commit())
        .as_ref()
        .expect("the third high-lane slot must be vacant");

    assert_eq!(queue.remove_if(0, |value| *value == 10), Some(10));
    assert_eq!(queue.remove_if(2, |value| *value == 12), Some(12));
    assert_eq!(queue.remove_if(1, |value| *value == 11), Some(11));
    assert!(queue.pop_front::<16>().is_none());
}

#[test]
fn fifo_layout_is_affine() {
    const CAPACITY: usize = 257;
    const NODE_BYTES: usize = 24;

    assert_eq!(std::mem::size_of::<Affine>(), 16);
    assert_eq!(
        std::mem::size_of::<lanes::Fifo<Affine, 2>>(),
        4 * std::mem::size_of::<usize>()
    );

    {
        let (empty, alloc) = Tracker::measure(|| {
            lanes::Fifo::<Affine, 2, _>::try_from(
                Count::<0>::try_new(capacity::Count::new(0)).expect("count fits bound"),
            )
        });
        assert_eq!(alloc, [0, 0, 0, 0]);
        let empty = (empty).expect("zero affine FIFO capacity must be representable");
        assert_eq!(
            std::mem::size_of_val(&empty),
            4 * std::mem::size_of::<usize>()
        );
    }

    let (queue, alloc) = Tracker::measure(|| {
        lanes::Fifo::<Affine, 2, _>::try_from(
            Count::<{ CAPACITY as u32 }>::try_new(capacity::Count::new(CAPACITY as u32))
                .expect("count fits bound"),
        )
    });
    assert_eq!(alloc, [1, CAPACITY * NODE_BYTES, 0, 0]);
    let mut queue = (queue).expect("the affine FIFO must allocate");

    let ((), alloc) = Tracker::measure(|| {
        for index in 0..CAPACITY {
            let value = Affine([index as u64, !(index as u64)]);
            if index & 1 == 0 {
                queue
                    .vacant::<0>(index)
                    .map(|vacant| vacant.insert::<false>(value).commit())
                    .as_ref()
                    .expect("each even affine slot must be inserted once");
            } else {
                queue
                    .vacant::<1>(index)
                    .map(|vacant| vacant.insert::<false>(value).commit())
                    .as_ref()
                    .expect("each odd affine slot must be inserted once");
            }
        }
        for index in (0..CAPACITY).step_by(2) {
            assert!(
                (queue.remove_if(index, |value| value.0[0] == index as u64))
                    == (Some(Affine([index as u64, !(index as u64)])))
            );
        }
        while queue.pop_front::<1>().is_some() {}
    });
    assert_eq!(alloc, [0, 0, 0, 0]);

    let ((), alloc) = Tracker::measure(move || {
        assert_eq!(
            std::mem::size_of_val(&queue),
            4 * std::mem::size_of::<usize>()
        );
    });
    assert_eq!(alloc, [0, 0, 1, CAPACITY * NODE_BYTES]);
}

#[test]
fn proofs_commit_once() {
    assert_eq!(
        std::mem::size_of::<
            o3::depot::queue::tagged::Vacant<
                o3::depot::queue::indexed::lanes::Position<
                    '_,
                    Affine,
                    2,
                    0,
                    o3::depot::capacity::Count,
                >,
            >,
        >(),
        2 * std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<
            o3::depot::queue::tagged::Linked<
                o3::depot::queue::indexed::lanes::Position<
                    '_,
                    Affine,
                    2,
                    0,
                    o3::depot::capacity::Count,
                    true,
                >,
                o3::depot::queue::indexed::lanes::Rollback,
            >,
        >(),
        2 * std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<
            Option<
                o3::depot::queue::tagged::Linked<
                    o3::depot::queue::indexed::lanes::Position<
                        '_,
                        Affine,
                        2,
                        0,
                        o3::depot::capacity::Count,
                        true,
                    >,
                    o3::depot::queue::indexed::lanes::Rollback,
                >,
            >,
        >(),
        2 * std::mem::size_of::<usize>()
    );

    let drops = cell::Cell::new(Some(0));
    let mut queue = (lanes::Fifo::<drops::Payload<'_>, 2, _>::try_from(
        Count::<7>::try_new(capacity::Count::new(7)).expect("count fits bound"),
    ))
    .expect("the proof-commit FIFO must allocate");

    {
        let proof = queue
            .vacant::<0>(6)
            .expect("queue entry must retain its original domain")
            .insert::<false>(drops::Payload {
                id: 60,
                drops: Counted(&drops),
            });
        assert_eq!(
            std::mem::size_of_val(&proof),
            2 * std::mem::size_of::<usize>()
        );
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(1));
    queue
        .vacant::<0>(6)
        .as_ref()
        .expect("required fixture outcome");

    for (index, id) in [(0, 10), (1, 11), (2, 12)] {
        queue
            .vacant::<0>(index)
            .expect("queue entry must retain its original domain")
            .insert::<false>(drops::Payload {
                id,
                drops: Counted(&drops),
            })
            .commit();
    }
    for (index, id) in [(3, 20), (4, 21), (5, 22)] {
        queue
            .vacant::<1>(index)
            .expect("queue entry must retain its original domain")
            .insert::<false>(drops::Payload {
                id,
                drops: Counted(&drops),
            })
            .commit();
    }

    assert!(queue.remove_if(1, |probe| probe.id == 99).is_none());
    for (index, id) in [(1, 11), (0, 10), (2, 12), (4, 21), (3, 20), (5, 22)] {
        let removed = queue
            .remove_if(index, |probe| probe.id == id)
            .expect("the exact fixture resident must be removable");
        assert_eq!(removed.id, id);
    }
    assert!(queue.pop_front::<0>().is_none());
    assert!(queue.pop_front::<1>().is_none());
    assert_eq!(drops.get(), Some(7));
}

#[test]
fn reuse_matches_models() {
    const CAPACITY: usize = 31;
    const STEPS: usize = 100_000;

    let mut queue = (lanes::Fifo::<Affine, 2, _>::try_from(
        Count::<{ CAPACITY as u32 }>::try_new(capacity::Count::new(CAPACITY as u32))
            .expect("count fits bound"),
    ))
    .expect("the model FIFO must allocate");
    let mut model = [VecDeque::new(), VecDeque::new()];
    let mut live = [None; CAPACITY];
    let mut epochs = [0_u64; CAPACITY];
    let mut rng = Random::<u64, 7, 9, 8>::try_new(0xd1b5_4a32_d192_ed03_u64).expect("shifts");

    for _ in 0..STEPS {
        let random = rng.advance();
        let index = random as usize % CAPACITY;
        let lane = (random >> 8) as usize & 1;

        match (random >> 16) & 3 {
            0 if live[index].is_none() => {
                epochs[index] += 1;
                let value = Affine([index as u64, epochs[index]]);
                if lane == 0 {
                    queue
                        .vacant::<0>(index)
                        .map(|vacant| vacant.insert::<false>(value).commit())
                        .as_ref()
                        .expect("the model proves the lane-zero slot vacant");
                } else {
                    queue
                        .vacant::<1>(index)
                        .map(|vacant| vacant.insert::<false>(value).commit())
                        .as_ref()
                        .expect("the model proves the lane-one slot vacant");
                }
                model[lane].push_back(index);
                live[index] = Some((lane, epochs[index]));
            }
            1 => {
                let want = model[lane].pop_front().map(|index| {
                    let (_, epoch) = live[index].take().expect("queued model resident");
                    Affine([index as u64, epoch])
                });
                let actual = if lane == 0 {
                    queue.pop_front::<0>()
                } else {
                    queue.pop_front::<1>()
                };
                assert!((actual) == (want));
            }
            2 => {
                let exact = live[index].map(|state| state.1);
                let want = exact.map(|epoch| {
                    let (lane, _) = live[index].take().expect("exact model resident");
                    let pos = model[lane]
                        .iter()
                        .position(|slot| *slot == index)
                        .expect("resident in lane model");
                    model[lane].remove(pos);
                    Affine([index as u64, epoch])
                });
                assert!((queue.remove_if(index, |value| Some(value.0[1]) == exact)) == (want));
            }
            _ => {}
        }

        for (index, state) in live.iter().enumerate() {
            assert_eq!(queue.vacant::<0>(index).is_ok(), state.is_none());
        }
    }
}

#[test]
fn destinations_are_exact() {
    let drops = Cell::new(Some(0));
    let first = Id::try_new(0, 1).expect("identity components fit");
    let second = Id::try_new(1, 1).expect("identity components fit");
    let ready = Index::at::<0>();
    let waiting = Index::at::<1>();
    let mut table = (lanes::Table::<Id<'static>, Owner<'_>, 2>::try_new(capacity::Count::new(2)))
        .expect("the two-slot lane transition table must allocate");
    table
        .vacant(first)
        .expect("first key is vacant")
        .insert(Owner {
            drops: Counted(&drops),
            value: 7,
        })
        .move_to_back(ready);
    table
        .vacant(second)
        .expect("second key is vacant")
        .insert(Owner {
            drops: Counted(&drops),
            value: 11,
        })
        .move_to_back(ready);

    let (removed, counts) = Tracker::measure(|| {
        let mut front = table.front(ready).expect("ready lane has its first entry");
        let value = front.get_mut();
        assert_eq!(value.value, 7);
        value.value = 9;
        assert!(table.front(ready).map(|front| front.key()) == Some(first));
        let stale = Id::try_new(0, 2).expect("identity components fit");
        assert!(table.vacant(stale).is_none());
        assert_eq!(drops.get(), Some(0));

        let front = table
            .front(ready)
            .expect("ready lane retains its first entry");
        front.move_to_back(waiting);
        assert!(table.front(ready).map(|front| front.key()) == Some(second));
        assert!(table.front(waiting).map(|front| front.key()) == Some(first));

        let front = table
            .front(ready)
            .expect("ready lane retains its second entry");
        front.unlink();

        let front = table
            .front(waiting)
            .expect("waiting lane retains its first entry");
        front.remove().value
    });

    assert_eq!(counts, [0; 4]);
    assert_eq!(removed, 9);
    assert!(table.is_vacant(ready));
    assert!(table.is_vacant(waiting));
    assert!(table.lane(second) == Some(None));
    assert!(table.vacant(first).is_some());
    assert_eq!(drops.get(), Some(1));
    let (seen, counts) = Tracker::measure(|| {
        let entry = table
            .entry(second)
            .expect("the retained key must remain occupied");
        entry.remove().value
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(seen, 11);
    assert_eq!(drops.get(), Some(2));
}
