pub mod common;

use common::alloc::system::{Activity, Tracker};
use o3::{
    alloc,
    depot::{
        self,
        capacity::{self, Count},
        queue::ring,
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn shared_ring_is_exact() {
    let (created, alloc) = Tracker::measure(|| ring::Cell::<[usize; 3]>::try_new(Count::new(3)));
    assert_eq!(alloc, [1, 72, 0, 0]);
    let queue = (created).expect("the measured three-slot shared ring must allocate once");

    let ((), hot) = Tracker::measure(|| {
        // SAFETY: this fixture inserts only two values into its empty three-slot cell.
        unsafe { depot::raw::RingCell::insert::<false>(&queue, [1, 0, 0]) };

        // SAFETY: this fixture inserts only two values into its empty three-slot cell.
        unsafe { depot::raw::RingCell::insert::<false>(&queue, [2, 0, 0]) };
        assert_eq!(queue.pop_front(), Some([1, 0, 0]));
        assert_eq!(queue.pop_front(), Some([2, 0, 0]));
    });
    assert_eq!(hot, [0; 4]);
    let ((), freed) = Tracker::measure(move || assert!(queue.is_empty()));
    assert_eq!(freed, [0, 0, 1, 72]);
}

#[test]
fn ring_allocates_once() {
    let (created, alloc) = Tracker::measure(|| ring::Fifo::<[usize; 3]>::try_new(Count::new(4)));
    assert_eq!(alloc, [1, 96, 0, 0]);
    let mut queue = (created).expect("the measured four-slot ring must allocate once");

    let ((), hot) = Tracker::measure(|| {
        queue
            .push_back([1, 0, 0])
            .expect("the ring back must be vacant");
        queue
            .vacant()
            .expect("the ring front must remain vacant")
            .push_front([0, 0, 0]);
        let front = (queue.front_entry()).expect("ring has a front");
        assert_eq!(front.get()[0], 0);
        let (value, vacant) = front.take();
        vacant.push_back(value);
        assert_eq!(queue.pop_front(), Some([1, 0, 0]));
        assert_eq!(queue.pop_front(), Some([0, 0, 0]));
    });
    assert_eq!(hot, [0; 4]);

    let ((), drops) = Tracker::measure(|| {
        let released = queue;
        assert!(released.is_empty());
    });
    assert_eq!(drops, [0, 0, 1, 96]);
}

#[test]
fn ring_initializes_once() {
    use std::num::NonZero;

    for capacity in [1, 4] {
        let capacity = NonZero::new(capacity).expect("positive ring extent");
        for first in [None, Some([7u64; 3])] {
            let (queue, activity) = Tracker::measure(|| ring::Fifo::try_with(capacity, first));
            let mut queue = queue.expect("one backing allocation");
            assert_eq!(activity, [1, capacity.get() as usize * 24, 0, 0]);
            assert_eq!(queue.capacity(), capacity.get() as usize);
            assert_eq!(queue.len(), usize::from(first.is_some()));
            assert_eq!(queue.pop_front(), first);
            queue
                .vacant()
                .expect("released first slot")
                .push_front([8; 3]);
            assert_eq!(queue.pop_front(), Some([8; 3]));
        }
    }
}

#[test]
fn ring_rejects_allocation() {
    use std::num::NonZero;

    let first = vec![7u8; 3];
    let (result, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || ring::Fifo::try_with(NonZero::<u32>::MIN, Some(first)))
    });
    assert!(matches!(result, Err(alloc::Error::Exhausted)));
    assert_eq!(activity, [0, 0, 1, 3]);
    let (queue, activity) =
        Tracker::measure(|| ring::Fifo::try_with(NonZero::<u32>::MAX, Some(())));
    let mut queue = queue.expect("ZST initialization needs no backing");
    assert_eq!(activity, [0; 4]);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.pop_front(), Some(()));
}

#[test]
fn rejects_without_effects() {
    let count = capacity::Count::new(4);

    let (fifo, trace) =
        Tracker::activity(|| Tracker::fail_after(0, || ring::Fifo::<u64>::try_new(count)));
    assert!(matches!(fifo, Err(alloc::Error::Exhausted)));
    assert!(trace == Activity::default());
}
