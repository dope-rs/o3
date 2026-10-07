use o3::depot::array::Index;
pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::mem;

use common::{alloc::system::Tracker, depot::index::key::Id};
use o3::{
    base::cell::region,
    depot::{
        capacity,
        map::direct,
        queue::indexed::lanes::{
            self, entry,
            runtime::{self, shared},
        },
    },
};

#[test]
fn runtime_alloc_is_exact() {
    region::Token::scope(|mut token| {
        const CAPACITY: usize = 257;
        const LANES: usize = 17;
        const NODE_BYTES: usize = 16;
        const HEAD_BYTES: usize = 8;

        {
            let (empty, alloc) = Tracker::measure(|| {
                shared::Fifo::<u64>::try_from(
                    capacity::Count::new(0),
                    capacity::Count::new(0),
                    &token,
                )
            });
            assert_eq!(alloc, [0, 0, 0, 0]);
            empty.expect("tagged queue domains must be disjoint");
        }

        {
            let (heads, alloc) = Tracker::measure(|| {
                shared::Fifo::<u64>::try_from(
                    capacity::Count::new(0),
                    capacity::Count::new(LANES as u32),
                    &token,
                )
            });
            assert_eq!(alloc, [1, LANES * HEAD_BYTES, 0, 0]);
            heads.expect("tagged queue domains must be disjoint");
        }

        let (queue, alloc) = Tracker::measure(|| {
            shared::Fifo::<u64>::try_from(
                capacity::Count::new(CAPACITY as u32),
                capacity::Count::new(LANES as u32),
                &token,
            )
        });
        assert_eq!(alloc, [1, CAPACITY * NODE_BYTES + LANES * HEAD_BYTES, 0, 0]);
        let queue = (queue).expect("tagged queue domains must be disjoint");

        let ((), alloc) = Tracker::measure(|| {
            for index in 0..CAPACITY {
                let lane = index % LANES;
                let head = queue.head(lane).expect("runtime lane exists");
                let mut lane = head.borrow(&mut token);
                lane.vacant(index)
                    .expect("each runtime node must be inserted once")
                    .insert::<false>(index as u64);
            }
            for index in 0..CAPACITY {
                assert!(
                    (queue
                        .head(0)
                        .map(|head| head.take(&mut token, index, index as u64)))
                        == (Ok(Some(index as u64)))
                );
            }
        });
        assert_eq!(alloc, [0, 0, 0, 0]);

        let ((), alloc) = Tracker::measure(move || {
            assert_eq!(
                std::mem::size_of_val(&queue),
                4 * std::mem::size_of::<usize>()
            );
        });
        assert_eq!(alloc, [0, 0, 1, CAPACITY * NODE_BYTES + LANES * HEAD_BYTES]);
    });
}

#[test]
fn runtime_nodes_compact() {
    const CAPACITY: usize = 257;
    const LANES: usize = 17;
    const NODE_BYTES: usize = 24;
    const HEAD_BYTES: usize = 8;

    {
        let (queue, alloc) = Tracker::measure(|| {
            runtime::Fifo::<[u64; 2]>::try_from(
                capacity::Count::new(CAPACITY as u32),
                capacity::Count::new(LANES as u32),
            )
        });
        assert_eq!(alloc, [1, CAPACITY * NODE_BYTES + LANES * HEAD_BYTES, 0, 0]);
        let queue = (queue).expect("tagged queue domains must be disjoint");

        assert_eq!(
            std::mem::size_of_val(&queue),
            3 * std::mem::size_of::<usize>()
        );
    }
}

#[test]
fn allocation_is_shared() {
    const LANES: usize = 3;
    let (table, counts) = Tracker::measure(|| {
        lanes::Table::<Id<'static>, u64, LANES>::try_new(capacity::Count::new(4))
    });
    assert_eq!(counts[0], 1);
    let mut table = (table).expect("the four-entry lane table must allocate");
    assert_eq!(mem::size_of_val(&table), 5 * mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<entry::Slot<'static, Id<'static>, u64, LANES>>(),
        3 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            entry::Slot<
                'static,
                Id<'static>,
                u64,
                LANES,
                direct::Vacant<'static, Id<'static>, lanes::Node<u64, LANES>>,
            >,
        >(),
        3 * mem::size_of::<usize>()
    );
    let first = Id::try_new(0, 1).expect("identity components fit");
    let second = Id::try_new(1, 1).expect("identity components fit");
    let third = Id::try_new(2, 1).expect("identity components fit");
    let ready = Index::at::<0>();
    let wait = Index::at::<1>();

    let (_, counts) = Tracker::measure(|| {
        let vacant = (table.vacant(first)).expect("first key is vacant");
        let entry = vacant.insert(10);
        assert!(entry.key() == first);
        assert!(entry.lane().is_none());
        assert_eq!(*entry.into_mut(), 10);
        table
            .vacant(second)
            .expect("the selected slot must be vacant")
            .insert(20)
            .unlink();
        table
            .vacant(third)
            .expect("the selected slot must be vacant")
            .insert(30)
            .unlink();
        for (key, lane) in [(first, ready), (second, ready), (third, wait)] {
            table
                .entry(key)
                .expect("inserted entry exists")
                .move_to_back(lane);
        }
        assert!(table.front(ready).map(|entry| entry.key()) == Some(first));
        table
            .front(ready)
            .expect("ready lane retains its front")
            .move_to_back(ready);
        assert!(table.front(ready).map(|entry| entry.key()) == Some(second));
        assert!(table.back(ready) == Some(first));
        assert_eq!(table.move_all(ready, wait), 2);
    });
    assert_eq!(counts, [0; 4]);

    assert!(table.front(wait).map(|entry| entry.key()) == Some(third));
    assert_eq!(table.front(wait).map(|entry| entry.remove()), Some(30));
    assert!(table.front(wait).map(|entry| entry.key()) == Some(second));
    assert!(table.back(wait) == Some(first));
}

#[test]
fn entries_are_atomic() {
    let mut table = (lanes::Table::<Id<'static>, usize, 2>::try_new(capacity::Count::new(2)))
        .expect("the two-entry lane table must allocate");
    let old = Id::try_new(0, 1).expect("identity components fit");
    let next = Id::try_new(0, 2).expect("identity components fit");
    let ready = Index::at::<0>();

    table
        .vacant(old)
        .expect("the selected slot must be vacant")
        .insert(7)
        .unlink();
    assert!(table.lane(old) == Some(None));
    table
        .entry(old)
        .expect("inserted generation exists")
        .move_to_back(ready);
    assert!(table.lane(old) == Some(Some(ready)));
    *table
        .entry(old)
        .expect("linked generation exists")
        .into_mut() = 8;
    assert_eq!(table.entry(old).map(|entry| entry.remove()), Some(8));
    assert!(table.lane(old).is_none());

    table
        .vacant(next)
        .expect("the selected slot must be vacant")
        .insert(9)
        .unlink();
    assert!(table.key_at(0) == Some(next));
    assert!(table.entry_at(0) == Some((next, &9)));
    assert_eq!(table.get(old), None);
    assert_eq!(table.get(next), Some(&9));
}

#[test]
fn removes_before_drop() {
    let mut table = (lanes::Table::<Id<'static>, Box<u64>, 1>::try_new(capacity::Count::new(2)))
        .expect("two occupied values must allocate");
    let first = Id::try_new(0, 1).expect("identity components fit");
    let second = Id::try_new(1, 1).expect("identity components fit");
    let lane = Index::at::<0>();
    for (key, value) in [(first, 7), (second, 11)] {
        let vacant = (table.vacant(key)).expect("distinct keys must be vacant");
        vacant.insert(Box::new(value)).move_to_back(lane);
    }
    let front = (table.front(lane)).expect("first value must be linked");
    let ((), counts) = Tracker::measure(|| {
        assert_eq!(*front.remove(), 7);
    });
    assert_eq!(counts, [0, 0, 1, mem::size_of::<u64>()]);
    assert!(table.front(lane).map(|front| front.key()) == Some(second));
    assert!(table.back(lane) == Some(second));
    assert!(table.get(first).is_none());
    assert_eq!(table.len(), 1);
    let ((), counts) = Tracker::measure(|| table.clear());
    assert_eq!(counts, [0, 0, 1, mem::size_of::<u64>()]);
    assert!(table.is_vacant(lane));
}

#[test]
fn retains_schedule() {
    let mut table = lanes::Table::<Id<'_>, u64, 2>::try_new(capacity::Count::new(2))
        .expect("lane table allocates");
    let key = Id::try_new(1, 7).expect("identity components fit");
    let ready = Index::at::<0>();
    let ((), counts) = Tracker::measure(|| {
        let vacant = (table.try_vacant(key)).expect("direct key must retain its complete identity");

        let mut occupied = vacant.insert(11);
        *occupied.get_mut() += 2;
        occupied.move_to_back(ready);
        let rejected = Id::try_new(1, 8).expect("identity components fit");
        assert!(table.try_vacant(rejected).is_err());
        let front = table
            .front(ready)
            .expect("the original schedule remains linked");
        assert!(front.key() == key);
        assert_eq!(front.remove(), 13);
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(table.len(), 0);
    assert!(table.is_vacant(ready));
    assert_eq!(
        mem::size_of::<
            lanes::entry::Slot<'_, Id<'_>, u64, 2, direct::Vacant<'_, Id<'_>, lanes::Node<u64, 2>>>,
        >(),
        3 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<
                lanes::entry::Slot<
                    '_,
                    Id<'_>,
                    u64,
                    2,
                    direct::Vacant<'_, Id<'_>, lanes::Node<u64, 2>>,
                >,
            >,
        >(),
        3 * mem::size_of::<usize>()
    );
}
