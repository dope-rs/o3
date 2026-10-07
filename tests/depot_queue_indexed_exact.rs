pub mod common;

use std::ops::ControlFlow;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use common::{alloc::system::Tracker, depot::model::Lcg};
use o3::depot::{
    capacity::Count,
    queue::{
        indexed::{exact, fifo},
        links,
    },
};

#[test]
fn coalesces_each_index() {
    let mut queue = (fifo::List::<u64, u32, exact::Policy>::try_count(Count::new(2)))
        .expect("the two-slot exact FIFO must allocate");
    assert_eq!(queue.len(), 0);
    let mut entry = queue.try_entry(1).expect("represented identity");
    entry.schedule(7);
    entry.schedule(7);
    entry.schedule(9);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.front(), Some(9));
    queue.try_entry(1).expect("represented identity").unlink(7);
    assert_eq!(queue.front(), Some(9));
    let mut entry = queue.try_entry(1).expect("represented identity");
    entry.unlink(9);
    entry.unlink(9);
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
}

#[test]
fn rejects_bounds() {
    for capacity in [0, 1, 3] {
        let mut queue =
            fifo::List::<u64, u32, exact::Policy>::try_new(capacity).expect("bounded queue");
        for index in [capacity as u32, u32::MAX] {
            let error = queue
                .try_entry(index)
                .err()
                .expect("index outside allocation");
            assert_eq!(error.index, index);
            assert_eq!(error.capacity, capacity);
            assert!(queue.is_empty());
        }
    }
}

#[test]
fn sweeps_original_heads() {
    let mut queue = fifo::List::<u32, links::Index, exact::Policy>::try_count(Count::new(3))
        .expect("three physical identities");
    for raw in 0..3 {
        let index = links::Index::try_new(raw).expect("compact identity");
        queue
            .try_entry(index)
            .expect("admission")
            .schedule(raw as u32);
    }
    let failed = queue.try_sweep(
        2,
        |_| Err::<(), _>(9),
        |(), _, _| ControlFlow::<()>::Break(()),
    );
    assert_eq!(failed, Err(9));
    assert_eq!(queue.len(), 3);
    let walked = queue
        .try_sweep(
            2,
            |count| {
                assert_eq!(count.get(), 2);
                Ok::<_, ()>(0)
            },
            |step, mut entry, value| {
                assert_eq!((entry.index().raw(), value), (step, step));
                if step == 0 {
                    entry.requeue(10);
                }
                ControlFlow::<(), _>::Continue(step + 1)
            },
        )
        .expect("admitted pass");
    assert_eq!(walked, Some(ControlFlow::Continue(2)));
    let walked = queue
        .try_sweep(
            usize::MAX,
            |count| {
                assert_eq!(count.get(), 2);
                Ok::<_, ()>(())
            },
            |(), mut entry, value| {
                assert_eq!((entry.index().raw(), value), (2, 2));
                entry.requeue(22);
                ControlFlow::Break(())
            },
        )
        .expect("partial pass");
    assert_eq!(walked, Some(ControlFlow::Break(())));
    assert_eq!(queue.pop_front(), Some(10));
    assert_eq!(queue.pop_front(), Some(22));
    let empty = queue.try_sweep(
        1,
        |_| Err::<(), ()>(()),
        |(), _, _| ControlFlow::<()>::Break(()),
    );
    assert_eq!(empty, Ok(None));
}

#[test]
fn preserves_order() {
    let mut queue =
        fifo::List::<_, u32, exact::Policy>::try_new(3).expect("three physical identities");
    for index in 0..3 {
        queue
            .try_entry(index)
            .expect("admission")
            .schedule((index, 1));
    }
    let mut entry = queue.try_entry(0).expect("admission");
    entry.schedule((0, 1));
    entry.unlink((0, 2));
    assert_eq!(queue.front(), Some((0, 1)));
    queue.try_entry(0).expect("admission").schedule((0, 2));
    assert_eq!(queue.pop_front(), Some((1, 1)));
    assert_eq!(queue.pop_front(), Some((2, 1)));
    assert_eq!(queue.pop_front(), Some((0, 2)));
    assert!(queue.is_empty());
}

#[test]
fn requeues_admitted_head() {
    let mut queue =
        fifo::List::<_, u32, exact::Policy>::try_new(3).expect("three physical identities");
    for index in 0..3 {
        queue
            .try_entry(index)
            .expect("admission")
            .schedule(index + 10);
    }
    queue.try_entry(0).expect("admission").requeue(10);
    assert_eq!(queue.front(), Some(11));
    let (source, value) = queue.pop().expect("queued head keeps its admission");
    assert_eq!(*source.key(), 1);
    assert_eq!(value, 11);
    let mut entry = source.into();
    entry.requeue(value);
    entry.requeue(value);
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.pop_front(), Some(12));
    assert_eq!(queue.pop_front(), Some(10));
    assert_eq!(queue.pop_front(), Some(11));
    assert!(queue.pop().is_none());
}

#[test]
fn matches_model() {
    use std::collections::VecDeque;

    let mut queue =
        fifo::List::<_, u32, exact::Policy>::try_new(7).expect("seven physical identities");
    let mut model = VecDeque::new();
    let mut rng = Lcg::<1>::new(1_u64);
    for _ in 0..50_000 {
        let random = rng.advance();
        let index = (random >> 32) as u32 % 7;
        let value = (index, (random >> 48) as u8);
        match random % 4 {
            0 | 1 => {
                let mut entry = queue.try_entry(index).expect("physical identity");
                entry.schedule(value);
                entry.schedule(value);
                if !model.contains(&value) {
                    model.retain(|&(stored, _)| stored != index);
                    model.push_back(value);
                }
            }
            2 => {
                queue
                    .try_entry(index)
                    .expect("physical identity")
                    .unlink(value);
                model.retain(|stored| *stored != value);
            }
            _ => assert_eq!(queue.pop_front(), model.pop_front()),
        }
        assert_eq!(queue.front(), model.front().copied());
        assert_eq!(queue.len(), model.len());
    }
}

#[test]
fn exact_allocates_once() {
    let (created, cold) = Tracker::measure(|| fifo::List::<u64, u32, exact::Policy>::try_new(4));
    let freed = {
        let mut queue = created.expect("four exact slots");
        assert_eq!(cold, [1, 64, 0, 0]);
        let ((), hot) = Tracker::measure(|| {
            let mut entry = queue.try_entry(2).expect("admitted slot");
            for value in 0..1_000 {
                entry.schedule(value);
                entry.schedule(value);
                entry.unlink(value + 1);
                entry.unlink(value);
            }
            entry.schedule(9);
            for _ in 0..1_000 {
                let (source, value) = queue.pop().expect("queued admission");
                let mut entry = source.into();
                entry.requeue(value);
                entry.requeue(value);
            }
            assert_eq!(queue.pop_front(), Some(9));
            let error = queue.try_entry(4).err().expect("out of bounds");
            assert_eq!((error.index, error.capacity), (4, 4));
        });
        assert_eq!(hot, [0; 4]);
        Tracker::begin()
    };
    assert_eq!(
        freed.finish(),
        common::alloc::system::Activity {
            allocations: 0,
            allocated_bytes: 0,
            deallocations: 1,
            deallocated_bytes: 64
        }
    );
}
