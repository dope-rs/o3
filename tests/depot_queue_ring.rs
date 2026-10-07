pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, collections::VecDeque, mem, num};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::model::{
        Lcg,
        drops::{Order, Tracked},
    },
};
use o3::depot::{
    self,
    capacity::Count,
    queue::ring::{self, inline},
};

#[test]
fn bounded_queues_are_fifo() {
    let mut queue =
        (ring::Fifo::try_new(Count::new(3))).expect("the three-slot FIFO ring must allocate");
    assert!(queue.is_empty());
    assert_eq!(queue.push_back(1), Ok(()));
    assert_eq!(queue.push_back(2), Ok(()));
    assert_eq!(queue.len(), 2);
    assert!((&queue).into_iter().any(|value| value == &1));
    assert_eq!(queue.pop_front(), Some(1));
    assert!(!(&queue).into_iter().any(|value| value == &1));
    assert_eq!(queue.push_back(3), Ok(()));
    assert_eq!(queue.push_back(4), Ok(()));
    assert!(queue.is_full());
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.push_back(5), Err(5));
    let full = queue.try_vacant();
    assert!(
        full.is_err_and(|full| (full.used(), full.capacity()) == (3, 3)),
        "full reservation must retain its occupancy"
    );
    assert_eq!(queue.pop_front(), Some(2));
    assert_eq!(queue.pop_front(), Some(3));
    assert_eq!(queue.pop_front(), Some(4));
    assert_eq!(queue.pop_front(), None);

    queue
        .try_vacant()
        .expect("drained queue has a vacancy")
        .push_back(6);
    assert_eq!(queue.pop_front(), Some(6));
}

#[test]
fn reserves_pop_vacancy() {
    let mut queue =
        (ring::Fifo::try_new(Count::new(2))).expect("the two-slot FIFO ring must allocate");
    assert_eq!(queue.push_back(1), Ok(()));
    assert_eq!(queue.push_back(2), Ok(()));

    let mut front = (queue.front_entry()).expect("an occupied ring has a front entry");
    *front.get_mut() = 3;
    let (value, vacant) = front.take();
    assert_eq!(value, 3);
    vacant.push_front(value);

    assert_eq!(queue.pop_front(), Some(3));
    assert_eq!(queue.pop_front(), Some(2));
    assert!(queue.front_entry().is_none());
}

#[test]
fn mutates_wrapped_ranges() {
    let mut empty = ring::Fifo::<u8>::try_new(Count::EMPTY).expect("empty queue storage");
    empty.clear();
    assert_eq!((&mut empty).into_iter().next(), None);
    assert_eq!(empty.as_slices(), (&[][..], &[][..]));
    let mut queue = ring::Fifo::try_new(Count::new(3)).expect("wrapped queue storage");
    for value in 0..3 {
        assert_eq!(queue.push_back(value), Ok(()));
    }
    assert_eq!(queue.pop_front(), Some(0));
    assert_eq!(queue.push_back(3), Ok(()));
    assert_eq!(queue.as_slices(), (&[1, 2][..], &[3][..]));
    {
        let mut values = (&mut queue).into_iter();
        *values.next().expect("wrapped front remains initialized") = 4;
        *values.nth(1).expect("wrapped back remains initialized") = 5;
    }
    for value in &mut queue {
        *value *= 2;
    }
    assert_eq!(queue.pop_front(), Some(8));
    assert_eq!(queue.pop_front(), Some(4));
    assert_eq!(queue.pop_front(), Some(10));
    assert_eq!((&mut queue).into_iter().next(), None);
    assert_eq!(queue.as_slices(), (&[][..], &[][..]));
    for (len, first) in [(3, 2), (1, 1), (3, 1)] {
        queue.clear();
        for value in 0..len {
            assert_eq!(queue.push_back(value), Ok(()));
        }
        assert_eq!(
            queue.as_slices(),
            (&[0, 1, 2][..first], &[0, 1, 2][first..len])
        );
    }
}

#[test]
fn handles_zst_capacity() {
    let mut queue = (ring::Fifo::<()>::try_new(Count::MAX))
        .expect("the maximum-capacity ZST ring must be allocation-free");
    assert!(queue.vacant().map(|vacant| vacant.push_front(())).is_some());
    assert_eq!(queue.push_back(()), Ok(()));
    assert_eq!(queue.len(), 2);
    assert_eq!((&mut queue).into_iter().count(), 2);
    assert_eq!(queue.pop_front(), Some(()));
    assert_eq!(queue.pop_front(), Some(()));
}

#[test]
fn matches_ring_model() {
    let mut fixed = (ring::Fifo::<u32>::try_new(Count::new(17)))
        .expect("the seventeen-slot ring model must allocate");
    let mut model = VecDeque::with_capacity(17);
    let mut rng = Lcg::<1>::new(1u64);
    for _ in 0..10_000 {
        let state = rng.advance();
        match (state >> 61) % 3 {
            0 => {
                let value = state as u32;
                let actual = fixed.push_back(value);
                let want = if model.len() == 17 {
                    Err(value)
                } else {
                    model.push_back(value);
                    Ok(())
                };
                assert_eq!(actual, want);
            }
            1 => {
                let value = state as u32;
                let actual = fixed.vacant().map(|vacant| vacant.push_front(value));
                let want = if model.len() == 17 {
                    None
                } else {
                    model.push_front(value);
                    Some(())
                };
                assert_eq!(actual, want);
            }
            _ => assert_eq!(fixed.pop_front(), model.pop_front()),
        }
        assert_eq!(fixed.len(), model.len());
        assert!((&fixed).into_iter().eq(model.iter()));
        assert_eq!(fixed.front(), model.front());
        assert_eq!(fixed.is_empty(), model.is_empty());
        assert_eq!(fixed.is_full(), model.len() == 17);
    }
}

#[test]
fn drops_wrapped_prefix() {
    for clear in [false, true] {
        let order = Cell::new(Some(0));
        {
            let mut queue = ring::Fifo::try_new(Count::new(4)).expect("wrapped drop ring");
            for value in 1..=4 {
                queue
                    .push_back(Order(&order, value))
                    .expect("available slot");
            }
            for _ in 0..3 {
                assert!(queue.pop_front().is_some());
            }
            assert_eq!(order.get(), Some(123));
            for value in 5..=6 {
                queue
                    .push_back(Order(&order, value))
                    .expect("available slot");
            }
            assert_eq!(order.get(), Some(123));
            if clear {
                queue.clear();
                assert!(queue.is_empty());
                assert_eq!(order.get(), Some(123456));
                queue
                    .vacant()
                    .expect("cleared slot")
                    .push_front(Order(&order, 7));
            }
        }
        assert_eq!(order.get(), Some(if clear { 1234567 } else { 123456 }));
    }
}

#[test]
fn rejects_reservation() {
    let (pool, counts) = Tracker::measure(|| ring::Pool::<u8>::try_new(Count::EMPTY));
    assert_eq!(counts, [0; 4]);
    let pool = pool.expect("empty allocation");
    let error = pool.try_reserve();
    assert!(
        error.is_err_and(|error| (error.used(), error.capacity()) == (0, 0)),
        "empty domain"
    );
    assert!(pool.take_front().is_none());
    let rejected = Tracker::fail_after(0, || ring::Pool::<u64>::try_new(Count::new(4)));
    assert!(matches!(rejected, Err(o3::alloc::Error::Exhausted)));
}

#[test]
fn retains_capacity() {
    let pool = ring::Pool::try_new(Count::new(2)).expect("two slots");
    let first = pool.try_reserve().expect("first reservation");
    let second = pool.try_reserve().expect("second reservation");
    let error = pool.try_reserve();
    assert!(
        error.is_err_and(|error| (error.used(), error.capacity()) == (2, 2)),
        "leased capacity is unavailable"
    );
    assert!(pool.is_empty());
    second.push_back(2);
    first.push_front(1);
    let (value, lease) = pool.take_front().expect("first publication");
    assert_eq!(value, 1);
    assert_eq!(pool.len(), 1);
    assert!(pool.try_reserve().is_err());
    lease.push_back(value);
    for expected in [2, 1] {
        let (value, lease) = pool.take_front().expect("publication order");
        assert_eq!(value, expected);
        lease.release();
    }
    let first = pool.try_reserve().expect("released first slot");
    let second = pool.try_reserve().expect("released second slot");
    first.release();
    second.release();
}

#[test]
fn rotates_reservations() {
    for capacity in [2, 3, 63, 64, 65] {
        let (pool, usage) = Tracker::usage(|| ring::Pool::try_new(Count::new(capacity)));
        assert_eq!(usage, (1, capacity as usize * size_of::<u64>()));
        let pool = pool.expect("exact allocation");
        let mut model = VecDeque::from_iter(0..u64::from(capacity));
        for value in model.iter().copied() {
            pool.try_reserve()
                .expect("initial capacity")
                .push_back(value);
        }
        let (retained, lease) = pool.take_front().expect("retained front");
        assert_eq!(Some(retained), model.pop_front());
        let ((), counts) = Tracker::measure(|| {
            for step in 0..4096 {
                let (value, slot) = pool.take_front().expect("queued front");
                assert_eq!(Some(value), model.pop_front());
                assert!(pool.try_reserve().is_err());
                if step % 3 == 0 {
                    slot.push_front(value);
                    model.push_front(value);
                } else {
                    slot.push_back(value);
                    model.push_back(value);
                }
            }
            lease.push_front(retained);
            assert_eq!(pool.len(), capacity as usize);
            let (value, lease) = pool.take_front().expect("retained publication");
            assert_eq!(value, retained);
            lease.release();
        });
        assert_eq!(counts, [0; 4]);
        while let Some((value, lease)) = pool.take_front() {
            assert_eq!(Some(value), model.pop_front());
            lease.release();
        }
        assert!(model.is_empty());
    }
}

#[test]
fn releases_reservations() {
    let pool = ring::Pool::<()>::try_new(Count::new(1)).expect("zero-sized slot");
    for _ in 0..4096 {
        let lease = pool.try_reserve().expect("available reservation");
        lease.release();
    }
    let lease = pool.try_reserve().expect("retained reservation");
    assert!(pool.try_reserve().is_err());
    assert!(pool.is_empty());
    lease.release();
    pool.try_reserve().expect("released capacity").release();
}

#[test]
fn drops_queued_values_once() {
    let released = {
        let (value, released) = {
            let pool = ring::Pool::try_new(Count::new(4)).expect("owner");
            for value in 0..4 {
                pool.try_reserve()
                    .expect("reserved slot")
                    .push_back(Box::new(value));
            }
            let (value, lease) = pool.take_front().expect("detached value");
            lease.release();
            pool.try_reserve()
                .expect("released slot")
                .push_back(Box::new(4));
            (value, Tracker::begin())
        };
        assert_eq!(released.finish().deallocations, 5);
        assert_eq!(*value, 0);
        Tracker::begin()
    };
    assert_eq!(released.finish().deallocations, 1);
}

#[test]
fn transfers_each_slot_once() {
    let (pool, allocs) = Tracker::count(|| inline::Pool::<_, 3>::new([1, 2, 3]));
    assert_eq!(allocs, 0);
    let ((), allocs) = Tracker::count(|| {
        for expected in 1..=3 {
            let slot = (pool.try_acquire()).expect("each initial slot must be available");
            assert_eq!(*slot.get(), expected);
            slot.push_back(());
        }
        let full = pool.try_acquire();
        assert!(
            full.is_err_and(|full| full.capacity() == 3),
            "queued slots cannot be acquired again"
        );
        for expected in 1..=3 {
            let (mut slot, ()) = (pool.pop_front()).expect("pending slots retain FIFO order");
            assert_eq!(*slot.get(), expected);
            *slot.get_mut() += 10;
            slot.release();
        }
        assert!(pool.is_empty());
        assert!(pool.pop_front().is_none());
        for expected in 11..=13 {
            let slot = (pool.try_acquire()).expect("every released slot must be available");
            assert_eq!(*slot.get(), expected);
            slot.push_back(());
        }
        for turn in 0..4096 {
            let (slot, ()) = (pool.pop_front()).expect("rotation stays nonempty");
            assert_eq!(*slot.get(), 11 + turn % 3);
            slot.push_back(());
        }
    });
    assert_eq!(allocs, 0);
}

#[test]
fn reserves_detached_slots() {
    let pool = inline::Pool::<_, 2>::new([1, 2]);
    let first = (pool.try_acquire()).expect("first slot exists");
    let second = (pool.try_acquire()).expect("second slot exists");
    assert!(pool.try_acquire().is_err());
    assert!(pool.is_empty());
    second.push_back(());
    first.push_back(());
    for expected in [2, 1] {
        let (slot, ()) = (pool.pop_front()).expect("both detached slots were queued");
        assert_eq!(*slot.get(), expected);
        slot.release();
    }
}

#[test]
fn borrows_without_growth() {
    let value = [1, 2, 3];
    let pool = inline::Pool::<_, 1>::new([value.as_slice()]);
    let slot = (pool.try_acquire()).expect("borrowed slot exists");
    assert_eq!(*slot.get(), value.as_slice());
    assert_eq!(mem::size_of_val(&slot), 2 * mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<Option<inline::Lease<'_, &[u8], 1>>>(),
        mem::size_of_val(&slot)
    );
    slot.release();
    assert_eq!(
        mem::size_of::<inline::Pool<usize, 64>>(),
        132 * mem::size_of::<usize>()
    );
    assert!(!mem::needs_drop::<inline::Pool<usize, 64>>());
}

#[test]
fn rejects_empty_pool() {
    let pool = inline::Pool::<usize, 0>::new([]);
    let full = pool.try_acquire();
    assert!(
        full.is_err_and(|full| full.capacity() == 0),
        "zero-capacity pool has no slots"
    );
    assert!(pool.is_empty());
    assert!(pool.pop_front().is_none());
}

#[test]
fn drops_payloads_once() {
    let drops = Cell::new(Some(0));
    {
        let pool = inline::Pool::<_, 3>::new([Counted(&drops), Counted(&drops), Counted(&drops)]);
        let slot = (pool.try_acquire()).expect("first slot exists");
        slot.push_back(());
        let slot = (pool.try_acquire()).expect("second slot exists");
        drop(slot);
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(3));
}

#[test]
fn retains_nonzero_payload() {
    let pool = inline::Pool::<_, 1, num::NonZero<u32>>::new([17]);
    let slot = (pool.try_acquire()).expect("initial slot");
    slot.push_back(num::NonZero::<u32>::MIN);
    let (slot, length) = (pool.pop_front()).expect("queued payload");
    assert_eq!(length, num::NonZero::<u32>::MIN);
    assert_eq!(*slot.get(), 17);
    slot.release();
}

#[test]
fn retains_fifo() {
    let queue =
        (ring::Cell::try_new(Count::new(3))).expect("the three-slot cell ring must allocate");

    for (values, front) in [([1, 2], 1), ([3, 4], 2)] {
        for value in values {
            // SAFETY: each batch leaves at most three initialized slots before its removal.
            unsafe { depot::raw::RingCell::insert::<false>(&queue, value) };
        }
        assert_eq!(queue.pop_front(), Some(front));
    }
    assert_eq!(queue.pop_front(), Some(3));
    assert_eq!(queue.pop_front(), Some(4));
    assert_eq!(queue.pop_front(), None);
}

#[test]
fn drops_cell_once() {
    for clear in [false, true] {
        let drops = [Cell::new(Some(0)), Cell::new(Some(0))];
        {
            let queue = (ring::Cell::try_new(Count::new(2)))
                .expect("the two-slot owned cell ring must allocate");
            for id in 0..2 {
                // SAFETY: this loop inserts exactly two values into this empty two-slot cell.
                unsafe {
                    depot::raw::RingCell::insert::<false>(
                        &queue,
                        Tracked::try_new(id, &drops).expect("drop counter exists"),
                    )
                };
            }
            assert_eq!(drops.each_ref().map(Cell::get), [Some(0); 2]);
            if clear {
                queue.clear();
                assert!(queue.is_empty());
                assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 2]);
            }
        }
        assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 2]);
    }
}
