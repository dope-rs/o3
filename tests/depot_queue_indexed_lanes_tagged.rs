pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{
    cell::{self, Cell},
    mem,
};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{index::capacity::Count, model::drops::Order, queue::indexed::slab::layout::Aligned},
};
use o3::{
    alloc,
    base::cell::region,
    depot::{
        self, capacity,
        queue::{
            indexed::lanes::{
                self, entry,
                runtime::{self, shared},
            },
            links, tagged,
        },
    },
};

#[test]
fn drops_in_lane_order() {
    region::Token::scope(|mut token| {
        let fixed = Cell::new(Some(0));
        let dynamic = Cell::new(Some(0));
        {
            let mut queue = lanes::Fifo::<Order<'_>, 2, _>::try_from(
                Count::<8>::try_new(capacity::Count::new(8)).expect("count fits bound"),
            )
            .expect("the sparse queues must allocate");
            let runtime = shared::Fifo::<Order<'_>>::try_from(
                capacity::Count::new(8),
                capacity::Count::new(2),
                &token,
            );
            let runtime = runtime.expect("the runtime queue must be available");
            queue
                .vacant::<1>(0)
                .expect("the first queue entry must be vacant")
                .insert::<false>(Order(&fixed, 3))
                .commit();
            queue
                .vacant::<0>(5)
                .expect("the second queue entry must be vacant")
                .insert::<false>(Order(&fixed, 1))
                .commit();
            queue
                .vacant::<0>(2)
                .expect("the third queue entry must be vacant")
                .insert::<false>(Order(&fixed, 2))
                .commit();
            for (lane, index, value) in [(1, 0, 3), (0, 5, 1), (0, 2, 2)] {
                let head = runtime.head(lane);
                let head = head.expect("the queue lane must be available");

                let mut lane = head.borrow(&mut token);
                lane.vacant(index)
                    .expect("the queue entry must be available")
                    .insert::<false>(Order(&dynamic, value));
            }
        }
        assert_eq!(fixed.get(), Some(123));
        assert_eq!(dynamic.get(), Some(123));
    });
}

#[test]
fn rejects_shape() {
    assert!(matches!(
        runtime::Fifo::<u8>::try_from(capacity::Count::MAX, capacity::Count::new(3)),
        Err(tagged::Error::Shape(tagged::shape::Error::Overlap {
            capacity: actual,
            lanes,
        })) if actual == capacity::Count::MAX && lanes == capacity::Count::new(3)
    ));
}

#[test]
fn rejects_exact_membership() {
    let mut fifo = (lanes::Fifo::<u32, 17, _>::try_from(
        Count::<2>::try_new(capacity::Count::new(2)).expect("count fits bound"),
    ))
    .expect("two slots must allocate");
    let vacant = match fifo.entry::<16>(1) {
        Ok(entry::State::Vacant(vacant)) => Some(vacant),
        _ => None,
    }
    .expect("the initial slot is vacant");
    vacant.insert::<false>(10).commit();
    assert!(matches!(
        fifo.entry::<0>(1),
        Ok(entry::State::Occupied(&10))
    ));
    assert!(matches!(
        fifo.entry::<16>(1),
        Ok(entry::State::Occupied(&10))
    ));
    assert!(matches!(
        fifo.vacant::<0>(1),
        Err(tagged::entry::Error::Linked { index, previous, next })
            if index.get() == 1 && previous == u32::MAX - 16 && next == u32::MAX - 16
    ));
    assert!(fifo.pop_front::<0>().is_none());
    assert_eq!(
        tagged::Endpoint::constant::<16>(&mut fifo).pop_entry(),
        Some((1, 10))
    );
    assert!(fifo.pop_front::<16>().is_none());
    assert!(matches!(
        fifo.entry::<16>(2),
        Err(entry::Error { index: 2, capacity: actual })
            if depot::raw::Count::count(actual) == capacity::Count::new(2)
    ));
    assert!(matches!(
        fifo.vacant::<16>(2),
        Err(tagged::entry::Error::Index {
            index: 2,
            capacity: actual
        }) if depot::raw::Count::count(actual) == capacity::Count::new(2)
    ));
}

#[test]
fn repairs_rollback() {
    let mut fifo = (lanes::Fifo::<u32, 2, _>::try_from(
        Count::<4>::try_new(capacity::Count::new(4)).expect("count fits bound"),
    ))
    .expect("four slots must allocate");
    fifo.vacant::<1>(0)
        .expect("the first queue entry must be vacant")
        .insert::<false>(10)
        .commit();
    fifo.vacant::<1>(1)
        .expect("the second queue entry must be vacant")
        .insert::<false>(20)
        .commit();
    let vacant = fifo
        .vacant::<1>(2)
        .expect("the queue entry must be available");

    assert_eq!(
        o3::depot::raw::TagEntry::index(vacant.insert::<false>(30).entry()).get(),
        2
    );
    assert_eq!(fifo.remove_if(0, |value| *value == 10), Some(10));
    assert_eq!(fifo.pop_front::<1>(), Some(20));
    fifo.vacant::<0>(2)
        .expect("the queue entry must be available")
        .insert::<false>(50)
        .commit();
    assert_eq!(fifo.pop_front::<0>(), Some(50));
    assert!(fifo.pop_front::<0>().is_none() && fifo.pop_front::<1>().is_none());
}

#[test]
fn retains_runtime_owner() {
    use lanes::runtime::shared;
    use o3::base::cell::region;

    let words = 2 * size_of::<usize>();
    assert_eq!(
        size_of::<
            o3::depot::queue::tagged::Endpoint<
                &'_ mut o3::depot::queue::indexed::lanes::runtime::Fifo<u32>,
            >,
        >(),
        words
    );
    assert_eq!(
        size_of::<
            Option<
                o3::depot::queue::tagged::Endpoint<
                    &'_ mut o3::depot::queue::indexed::lanes::runtime::Fifo<u32>,
                >,
            >,
        >(),
        words
    );
    assert_eq!(
        size_of::<
            o3::depot::queue::tagged::Vacant<
                o3::depot::queue::tagged::Endpoint<
                    &'_ mut o3::depot::queue::indexed::lanes::runtime::Fifo<u32>,
                    links::Index,
                    false,
                >,
            >,
        >(),
        words
    );
    assert_eq!(
        size_of::<
            Option<
                o3::depot::queue::tagged::Vacant<
                    o3::depot::queue::tagged::Endpoint<
                        &'_ mut o3::depot::queue::indexed::lanes::runtime::Fifo<u32>,
                        links::Index,
                        false,
                    >,
                >,
            >,
        >(),
        words
    );
    region::Token::scope(|mut token| {
        let left =
            shared::Fifo::<u32>::try_from(capacity::Count::new(2), capacity::Count::new(2), &token);
        let left = left.expect("the left queue must be available");
        let right =
            shared::Fifo::<u32>::try_from(capacity::Count::new(2), capacity::Count::new(1), &token);
        let right = right.expect("the right queue must be available");
        assert!(matches!(
            left.head(2),
            Err(tagged::lane::Error { lane: 2, lanes: actual })
                if actual == capacity::Count::new(2)
        ));
        let first = left.head(1);
        let first = first.expect("the first lane must be available");
        let second = right.head(0);
        let second = second.expect("the second lane must be available");
        {
            let mut guard = first.borrow(&mut token);
            assert!(matches!(
                guard.vacant(2),
                Err(tagged::entry::Error::Index {
                    index: 2,
                    capacity: actual
                }) if actual == capacity::Count::new(2)
            ));
            guard
                .vacant(0)
                .expect("the queue entry must be available")
                .insert::<false>(11);
        }
        {
            let mut guard = second.borrow(&mut token);
            guard
                .vacant(0)
                .expect("the queue entry must be available")
                .insert::<false>(22);
        }
        assert!(
            matches!(first.borrow(&mut token).vacant(0), Err(tagged::entry::Error::Linked { index, previous, next }) if index.get() == 0 && previous == u32::MAX - 1 && next == u32::MAX - 1)
        );
        assert_eq!(first.take(&mut token, 0, 22), None);
        assert_eq!(first.borrow(&mut token).pop_front(), Some(11));
        let mut guard = {
            let head = second;
            head.borrow(&mut token)
        };
        assert_eq!(guard.pop_front(), Some(22));
    });
}

#[test]
fn layouts_drop_once() {
    region::Token::scope(|mut token| {
        for (capacity, lanes, bytes) in [(0, 0, 0), (0, 3, 128), (3, 0, 768), (3, 2, 896)] {
            let drops = cell::Cell::new(Some(0));
            let (built, cold) = Tracker::measure(|| {
                shared::Fifo::<Aligned<'_>>::try_from(
                    capacity::Count::new(capacity as u32),
                    capacity::Count::new(lanes as u32),
                    &token,
                )
            });
            assert_eq!(cold, [usize::from(bytes != 0), bytes, 0, 0]);
            let freed = {
                let queue = (built).expect("tagged queue domains must be disjoint");

                assert_eq!(mem::size_of_val(&queue), 4 * mem::size_of::<usize>());
                if lanes != 0 {
                    for index in 0..capacity {
                        let head = (queue.head(index % lanes))
                            .expect("queue lane must be in its original domain");

                        let mut lane = head.borrow(&mut token);
                        let vacant = (lane.vacant(index))
                            .expect("queue entry must retain its original domain");

                        vacant.insert::<false>(Aligned(Counted(&drops)));
                    }
                }
                assert_eq!(drops.get(), Some(0));
                Tracker::begin()
            };
            assert_eq!(
                freed.finish(),
                common::alloc::system::Activity {
                    deallocations: usize::from(bytes != 0),
                    deallocated_bytes: bytes,
                    ..Default::default()
                }
            );
            assert_eq!(drops.get(), Some(if lanes == 0 { 0 } else { capacity }));
        }
    });
}

#[test]
fn failure_is_inert() {
    let (failed, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            runtime::Fifo::<Aligned<'_>>::try_from(capacity::Count::new(3), capacity::Count::new(2))
        })
    });
    assert!(matches!(
        failed,
        Err(tagged::Error::Allocation(alloc::Error::Exhausted))
    ));
    assert_eq!(activity, [0; 4]);
}
