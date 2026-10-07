pub mod common;

use std::{cell, mem, pin::Pin, ptr};

use common::{
    alloc::system::Tracker,
    depot::{
        arena::pool::Pinned,
        model::drops::Tracked,
        queue::{Error, Source},
    },
};
use o3::{
    depot::{
        self,
        arena::linked::slots,
        queue::pool::{self, fifo},
        raw::Proof,
    },
    mem::Return as _,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn move_keeps_address() {
    let plan = pool::Plan::new(1, 1).expect("one lane and slot");
    let mut lanes = pool::Lanes::<Pinned>::try_new(plan).expect("one resident allocation");
    assert!(
        lanes
            .get_mut(0)
            .expect("lane zero")
            .push_back(Pinned::default())
            .is_ok()
    );
    let address = {
        let resident = lanes
            .get(0)
            .expect("lane zero")
            .front()
            .expect("resident front");
        resident.value.set(7);
        ptr::from_ref(resident)
    };
    let mut moved = Some(lanes);
    let lanes = Pin::new(&mut moved)
        .get_mut()
        .as_mut()
        .expect("moved owner");
    let resident = lanes
        .get(0)
        .expect("lane zero")
        .front()
        .expect("same resident");
    assert_eq!(ptr::from_ref(resident), address);
    assert_eq!(resident.value.get(), 7);
    let resident = lanes
        .get_mut(0)
        .expect("lane zero")
        .pop_front()
        .expect("movable resident");
    assert_eq!(resident.value.get(), 7);
}

#[test]
fn admission_is_atomic() {
    let plan = pool::Plan::new(4, 2).expect("four slots and two lanes");
    let mut queues = pool::Lanes::<_>::try_new(plan).expect("queue allocation");
    let started = cell::Cell::new(Some(0));
    {
        let mut lanes = queues.iter_mut();
        let mut first = lanes.next().expect("first lane");
        let mut second = lanes.next().expect("second lane");
        assert!(
            first
                .try_extend(Source::try_new([10, 11, 12], &started).expect("count"))
                .expect("first flight fits")
                .as_slice()
                .is_empty()
        );
        let input = Source::try_new([20, 21], &started).expect("count");
        let Error { input, cause } = second
            .try_extend(input)
            .expect_err("whole flight cannot fit");
        assert_eq!(cause.requested.get(), 2);
        assert_eq!(cause.available, 1);
        assert_eq!(input.items, [20, 21]);
        assert_eq!(started.get(), Some(1));
        assert_eq!(first.pop_front(), Some(10));
        assert!(
            second
                .try_extend(input)
                .expect("released slot admits the flight")
                .as_slice()
                .is_empty()
        );
        assert_eq!([first.pop_front(), first.pop_front()], [Some(11), Some(12)]);
        assert_eq!(
            [second.pop_front(), second.pop_front()],
            [Some(20), Some(21)]
        );
        assert!(first.is_empty() && second.is_empty());
    }
    assert_eq!(queues.available(), 4);
}

#[test]
fn orders_limit() {
    assert!(matches!(
        pool::Plan::new(usize::MAX, usize::MAX),
        Err(pool::Error::Slots {
            requested: usize::MAX
        })
    ));
    assert!(matches!(
        pool::Plan::new(0, usize::MAX),
        Err(pool::Error::Lanes {
            requested: usize::MAX
        })
    ));
    let count = u32::MAX as usize;
    let plan = pool::Plan::new(count, count).expect("full domains");
    assert_eq!((plan.slots(), plan.lanes()), (count, count));
    if usize::BITS > u32::BITS {
        let plan = pool::Plan::new(0, count).expect("full lane domain");
        let result = Tracker::fail_after(0, || pool::Lanes::<()>::try_new(plan));
        assert!(matches!(result, Err(o3::alloc::Error::Exhausted)));
    }
}

#[test]
fn skips_forgotten_removal() {
    let drops = [cell::Cell::new(Some(0))];
    let plan = (pool::Plan::new(2, 1)).expect("two slots and one lane must fit");
    {
        let mut lanes =
            (pool::Lanes::<Tracked<'_>>::try_new(plan)).expect("the lane must allocate");
        let mut lane = lanes.try_bind_mut(0).expect("allocated lane");
        assert!(
            lane.push_back(Tracked::try_new(0, &drops).expect("drop counter exists"))
                .is_ok()
        );
        let removed = lane.front_entry().expect("initialized front").remove();
        let (vacant, value) = removed.into_parts();
        drop(value);
        assert_eq!(drops[0].get(), Some(1));
        mem::forget(vacant);
    }
    assert_eq!(drops[0].get(), Some(1));
}

#[test]
fn rejects_missing_lanes() {
    let plan = pool::Plan::new(3, 2).expect("two lanes");
    let mut lanes = pool::Lanes::<u64>::try_new(plan).expect("allocated lanes");
    lanes
        .try_bind_mut(1)
        .expect("last lane")
        .push_back(7)
        .expect("free node");
    for index in [2, usize::MAX] {
        let error = lanes.try_bind(index).err().expect("outside lane domain");
        assert_eq!(error, pool::Missing { index, lanes: 2 });
        let error = lanes
            .try_bind_mut(index)
            .err()
            .expect("outside lane domain");
        assert_eq!(error, pool::Missing { index, lanes: 2 });
    }
    assert_eq!(lanes.available(), 2);
    assert!(lanes.try_bind(0).expect("first lane").is_empty());
    assert_eq!(lanes.try_bind(1).expect("last lane").front(), Some(&7));
}

#[test]
fn reuses_bound_lane() {
    let plan = pool::Plan::new(3, 2).expect("two lanes");
    let mut lanes = pool::Lanes::<u64, u64>::try_new(plan).expect("allocated lanes");
    let ((), delta) = Tracker::measure(|| {
        let mut lane = lanes.try_bind_mut(1).expect("last lane");
        for value in [3, 5, 7] {
            lane.push_back(value).expect("free node");
        }
        *lane.state_mut() = 11;
        let mut front = lane.borrow_mut().front_entry().expect("nonempty lane");
        let (value, state) = front.parts_mut();
        *value += *state;
        *state += 2;
        assert_eq!(front.pop(), 14);
        assert_eq!(lane.pop_front(), Some(5));
        assert_eq!(lane.pop_front(), Some(7));
        assert!(lane.front_mut().is_none());
        assert_eq!(*lane.state(), 13);
        let state = pool::View::from(lane).state();
        assert_eq!(*state, 13);
    });
    assert_eq!(delta, [0; 4]);
    assert_eq!(lanes.available(), 3);
    assert_eq!(*lanes.try_bind(0).expect("first lane").state(), 0);
}

#[test]
fn borrowed_lane_indices() {
    let plan = pool::Plan::new(2, 2).expect("two lanes");
    let mut lanes =
        pool::Lanes::<usize, _>::try_from_fn(plan, |index| index).expect("allocated lanes");
    for mut lane in lanes.iter_mut() {
        lane.push_back(lane.state().get()).expect("free node");
    }
    assert_eq!(
        lanes
            .iter_mut()
            .map(|mut lane| lane.pop_front())
            .collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
}

#[test]
fn retains_head_transition() {
    let plan = pool::Plan::new(2, 1).expect("one lane");
    let mut lanes = pool::Lanes::<u64>::try_new(plan).expect("lane allocation");
    let mut lane = lanes.try_bind_mut(0).expect("only lane");
    lane.push_back(3).expect("first slot");
    lane.push_back(5).expect("second slot");
    let (lane, head) = lane.front_entry().expect("nonempty lane").into_parts();
    // SAFETY: this exact lane and its current head remain borrowed and unchanged.
    let front = pool::Front::from_source(unsafe { Proof::new((lane, head)) });
    let (lane, ()) = front.remove_with(|removed| drop(removed));
    let front = lane.front_entry().expect("restored head");
    let (mut lane, value) = front.remove_with(|removed| {
        let (vacant, value) = removed.into_parts();
        drop(vacant);
        value
    });
    assert_eq!(value, 3);
    assert_eq!(lane.pop_front(), Some(5));
    assert!(lane.is_empty());
}

#[test]
fn mutate_front_in_place() {
    let plan =
        (pool::Plan::new(3, 2)).expect("three slots and two lanes must fit their index domains");
    let mut pool =
        (pool::Lanes::<String>::try_new(plan)).expect("the three-slot lane pool must allocate");
    {
        let mut lane = (pool.get_mut(0)).expect("the plan contains lane zero");
        lane.push_back(String::from("head"))
            .as_ref()
            .expect("required fixture outcome");
        lane.push_back(String::from("tail"))
            .as_ref()
            .expect("required fixture outcome");
    }
    let mut lane = (pool.get_mut(1)).expect("the plan contains lane one");
    lane.push_back(String::from("other"))
        .as_ref()
        .expect("required fixture outcome");

    let head = pool.get(0).and_then(|lane| lane.front());
    assert_eq!(head.map(String::as_str), Some("head"));
    let pointer = head.map(|value| value as *const String);
    let lane = (pool.get_mut(0)).expect("the plan retains lane zero");
    let mut front = (lane.front_entry()).expect("lane zero retains its head");
    front.get_mut().push_str("-mutated");

    let lane = (pool.get(0)).expect("the plan retains lane zero");
    assert_eq!(lane.front().map(String::as_str), Some("head-mutated"));
    assert_eq!(lane.front().map(|value| value as *const String), pointer);
    assert_eq!(lane.len(), 2);
    assert_eq!(pool.get(1).map(|lane| lane.len()), Some(1));
}

#[test]
fn borrow_front_before_pop() {
    let plan =
        (pool::Plan::new(2, 1)).expect("two slots and one lane must fit their index domains");
    let mut pool =
        (pool::Lanes::<i32>::try_new(plan)).expect("the two-slot lane pool must allocate");
    let mut lane = (pool.get_mut(0)).expect("the plan contains lane zero");
    lane.push_back(1)
        .as_ref()
        .expect("required fixture outcome");
    lane.push_back(2)
        .as_ref()
        .expect("required fixture outcome");

    assert_eq!(pool::View::from(&lane).front(), Some(&1));
    assert_eq!(lane.count().get(), 2);
    assert_eq!(lane.pop_front(), Some(1));
    assert_eq!(lane.pop_front(), Some(2));
}

#[test]
fn consume_front() {
    let plan =
        (pool::Plan::new(2, 1)).expect("two slots and one lane must fit their index domains");
    let mut pool =
        (pool::Lanes::<u8>::try_new(plan)).expect("the two-slot lane pool must allocate");
    let mut lane = (pool.get_mut(0)).expect("the plan contains lane zero");
    lane.push_back(1)
        .as_ref()
        .expect("required fixture outcome");
    lane.push_back(2)
        .as_ref()
        .expect("required fixture outcome");

    let mut front = (lane.front_entry()).expect("the lane contains its first value");
    *front.get_mut() = 3;
    let (value, lane) = front.take();
    assert_eq!(value, 3);
    assert_eq!(lane.count().get(), 1);
    let front = (lane.front_entry()).expect("the lane contains its second value");
    assert_eq!(front.pop(), 2);
    assert_eq!(pool.get(0).map(|lane| lane.is_empty()), Some(true));
}

#[test]
fn restore_or_discard_front() {
    let plan =
        (pool::Plan::new(2, 1)).expect("two slots and one lane must fit their index domains");
    let mut pool =
        (pool::Lanes::<String>::try_new(plan)).expect("the two-slot lane pool must allocate");
    let mut lane = (pool.get_mut(0)).expect("the plan contains lane zero");
    lane.push_back(String::from("first"))
        .as_ref()
        .expect("required fixture outcome");
    lane.push_back(String::from("second"))
        .as_ref()
        .expect("required fixture outcome");

    {
        let removed = lane
            .front_entry()
            .expect("the full lane exposes its first value")
            .remove();
        assert_eq!(removed.item(), "first");
    }
    let lane = (pool.get_mut(0)).expect("the plan contains lane zero");
    assert_eq!(
        pool::View::from(&lane).front().map(String::as_str),
        Some("first")
    );

    let removed = lane
        .front_entry()
        .expect("discard rollback leaves the first value at the front")
        .remove();
    let (vacant, value) = removed.into_parts();
    vacant.return_item(value + "-restored");
    let lane = (pool.get_mut(0)).expect("the plan contains lane zero");
    assert_eq!(
        pool::View::from(&lane).front().map(String::as_str),
        Some("first-restored")
    );

    {
        let removed = lane
            .front_entry()
            .expect("restoration leaves the restored value at the front")
            .remove();
        let removed = removed.into_parts();
        assert_eq!(removed.1, "first-restored");
    }
    let lane = (pool.get_mut(0)).expect("the plan contains lane zero");
    assert_eq!(
        pool::View::from(&lane).front().map(String::as_str),
        Some("second")
    );
}

#[test]
fn zero_lanes_are_inert() {
    let plan =
        (pool::Plan::new(0, 0)).expect("zero slots and zero lanes must fit their index domains");
    let mut pool = (pool::Lanes::<u8>::try_new(plan)).expect("the empty lane pool must allocate");

    assert_eq!(pool.available(), 0);
    assert!(pool.get_mut(0).is_none());
    assert!(pool.get(0).is_none());
}

#[test]
fn fifos_share_nodes() {
    const CAPACITY: usize = 4;
    const BYTES: usize = CAPACITY * 32;

    let (result, build) = Tracker::measure(|| depot::raw::Arena::<[usize; 3]>::try_new(CAPACITY));
    let arena = result.expect("the four-node FIFO arena must allocate");
    assert_eq!(build, [1, BYTES, 0, 0]);

    let ((), hot) = Tracker::measure(|| {
        {
            let mut second = arena.fifo();
            {
                let mut first = arena.fifo();
                first
                    .vacant()
                    .expect("planned FIFO vacancy")
                    .push_back([1, 0, 0]);
                first
                    .vacant()
                    .expect("planned FIFO vacancy")
                    .push_back([2, 0, 0]);
                second
                    .vacant()
                    .expect("planned FIFO vacancy")
                    .push_back([3, 0, 0]);
                assert_eq!(arena.available(), 1);
                let mut values = first.iter();
                assert_eq!(values.next(), Some(&[1, 0, 0]));
                assert_eq!(values.next(), Some(&[2, 0, 0]));
                assert!(values.next().is_none());
            }
            assert_eq!(arena.available(), 3);
        }

        let free = CAPACITY;
        {
            let mut reserved = arena.fifo();
            {
                let entry = reserved.vacant();
                assert!(entry.is_some(), "one vacant node");
                assert_eq!(arena.available(), free - 1);
            }
            assert_eq!(arena.available(), free);

            reserved
                .vacant()
                .expect("planned FIFO vacancy")
                .push_back([11, 0, 0]);
            assert_eq!(reserved.iter().next(), Some(&[11, 0, 0]));
            assert_eq!(arena.available(), free - 1);
        }
        assert_eq!(arena.available(), free);

        let mut reused = arena.fifo();
        for value in 0..free {
            reused
                .vacant()
                .expect("returned slot")
                .push_back([value, 0, 0]);
        }
        assert!(reused.vacant().is_none());

        if usize::BITS == 64 {
            assert_eq!(mem::size_of::<fifo::Fifo<'static, [usize; 3]>>(), 16);
            assert_eq!(
                mem::size_of::<
                    slots::Vacant<[usize; 3], &'static mut fifo::Fifo<'static, [usize; 3]>>,
                >(),
                16
            );
        }
    });
    assert_eq!(hot, [0; 4]);
}

#[test]
fn lanes_pack_states() {
    const CAPACITY: usize = 4;
    const LANES: usize = 2;
    const BYTES: usize = CAPACITY * 32 + LANES * mem::size_of::<slots::State>();

    let plan = (pool::Plan::new(CAPACITY, LANES))
        .expect("four node slots and two lanes must fit the u32 domain");
    let (result, build) = Tracker::measure(|| pool::Lanes::<[usize; 3]>::try_new(plan));
    let mut lanes = result.expect("the four-node two-lane arena must allocate");
    assert_eq!(build, [1, BYTES, 0, 0]);

    let ((), hot) = Tracker::measure(|| {
        let mut first = (lanes.get_mut(0)).expect("the first lane must exist");
        assert!(first.push_back([1, 0, 0]) == Ok(()));
        let vacant = (first.vacant()).expect("the first lane has a spare slot");
        vacant.return_item([0, 0, 0]);
        let mut front = (first.front_entry()).expect("the returned value must occupy the front");
        assert_eq!(front.get_mut()[0], 0);
        front.get_mut()[0] = 4;
        let (value, first) = front.take();
        assert_eq!(value, [4, 0, 0]);
        let front = (first.front_entry()).expect("the original lane must retain the second value");
        assert_eq!(front.pop(), [1, 0, 0]);
    });
    assert_eq!(hot, [0; 4]);

    if usize::BITS == 64 {
        assert_eq!(mem::size_of_val(&lanes), 40);
    }
    assert_eq!(
        mem::size_of::<pool::View<'static, u64>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<pool::Lane<'static, u64>>(),
        2 * mem::size_of::<usize>()
    );
    let ((), teardown) = Tracker::measure(move || drop(lanes));
    assert_eq!(teardown, [0, 0, 1, BYTES]);
}

#[test]
fn detached_fifo_recycles() {
    let arena = depot::raw::Arena::<[usize; 3]>::try_new(3usize).expect("three nodes");
    let mut queue = arena.fifo();
    for value in 0..3 {
        queue
            .vacant()
            .expect("planned FIFO vacancy")
            .push_back([value, 0, 0]);
    }
    {
        let front: fifo::Front<'_, '_, [usize; 3]> = queue.front().expect("nonempty queue");
        assert_eq!(front.get(), &[0, 0, 0]);
        let detached: fifo::Nonempty<'_, [usize; 3]> = front.take();
        assert!(queue.is_empty());
        assert!(queue.vacant().is_none());
        assert_eq!(
            detached.iter().map(|value| value[0]).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let (first, detached) = detached.pop();
        assert_eq!(first, [0, 0, 0]);
        queue
            .vacant()
            .expect("planned FIFO vacancy")
            .push_back([7, 0, 0]);
        let rest = detached.expect("two nodes").pop();
        assert_eq!(rest.0, [1, 0, 0]);
    }
    assert_eq!(queue.pop_front(), Some([7, 0, 0]));
    assert_eq!(arena.available(), 3);
    for value in 0..3 {
        queue
            .vacant()
            .expect("returned slot")
            .push_back([value, 0, 0]);
    }
    assert!(queue.vacant().is_none());
}

#[test]
fn forgotten_fifo_reclaimed() {
    let drops = [cell::Cell::new(Some(0)), cell::Cell::new(Some(0))];
    {
        let arena = depot::raw::Arena::<Tracked<'_>>::try_new(2usize).expect("two nodes");
        {
            let mut queue = arena.fifo();
            for index in 0..2 {
                queue
                    .vacant()
                    .expect("unused slot")
                    .push_back(Tracked::try_new(index, &drops).expect("drop counter exists"));
            }
            let detached = queue.front().expect("nonempty queue").take().pop().1;
            assert_eq!(drops[0].get(), Some(1));
            mem::forget(detached);
        }
    }
    assert_eq!(drops.each_ref().map(cell::Cell::get), [Some(1); 2]);
}
