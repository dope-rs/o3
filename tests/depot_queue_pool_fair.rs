pub mod common;

use common::alloc::system::Tracker;
use o3::{
    base::cell::region,
    depot::{
        self,
        queue::pool::{self, fair},
    },
    mem::credit::fair::{Axis, Cause},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn plan_bounds() {
    use pool::fair::{Error, Plan};

    let limit = u32::MAX as usize;
    for capacity in [0, 1, limit, usize::MAX] {
        assert!(
            matches!(Plan::new(capacity, 0), Err(Error::ZeroLanes { capacity: actual }) if actual == capacity)
        );
    }
    for (capacity, lanes) in [(0, 1), (1, 2), (limit - 1, limit)] {
        assert!(
            matches!(Plan::new(capacity, lanes), Err(Error::CapacityBelowLanes { capacity: actual, lanes: required }) if actual == capacity && required == lanes)
        );
    }
    for lanes in [1, limit / 2, limit] {
        Plan::new(limit, lanes)
            .as_ref()
            .expect("required fixture outcome");
        Plan::new(lanes, lanes)
            .as_ref()
            .expect("required fixture outcome");
    }
    if let Some(overflow) = limit.checked_add(1) {
        assert!(
            matches!(Plan::new(limit, overflow), Err(Error::CapacityBelowLanes { capacity, lanes }) if capacity == limit && lanes == overflow)
        );
        for capacity in [overflow, usize::MAX] {
            for lanes in [1, limit, capacity] {
                assert!(
                    matches!(Plan::new(capacity, lanes), Err(Error::Capacity(error)) if error.requested() == capacity)
                );
            }
        }
    }
}

#[test]
fn settles_permits() {
    region::Token::scope(|mut token| {
        let plan = (fair::Plan::new(2, 1)).expect("two slots must fit one lane");
        let lanes = (fair::Lanes::<u64>::try_new(plan, &token))
            .expect("the two-slot fair queue must allocate");
        let lane = (lanes.bind(0)).expect("the constructed lane must be addressable");
        let second = {
            let first = lane.reserve();
            assert!(first.is_ok(), "credit demand must fit its original lane");

            let second = (lane.reserve()).expect("credit demand must fit its original lane");

            let cause = lane
                .reserve()
                .err()
                .expect("outstanding permits exhaust admission");
            assert!(
                cause
                    == Cause::Exhausted {
                        axis: Axis::at::<0>(),
                        required: 1,
                        available: 0,
                    }
            );

            second
        };
        second.push_back(&mut token, 7);
        {
            let unused = lane.reserve();
            assert!(unused.is_ok(), "credit demand must fit its original lane");
        }
        assert_eq!(lane.view(&token).front(), Some(&7));

        let (value, permit) =
            (lane.take_front(&mut token)).expect("the published value must retain its permit");
        assert_eq!(value, 7);
        assert_eq!(lane.view(&token).len(), 0);
        permit.push_back(&mut token, 9);
        {
            let retired =
                (lane.take_front(&mut token)).expect("the detached permit must publish again");
            assert_eq!(retired.0, 9);
        }

        for value in [11, 13] {
            let permit = (lane.reserve()).expect("credit demand must fit its original lane");

            permit.push_back(&mut token, value);
        }
        for expected in [11, 13] {
            let retired =
                (lane.take_front(&mut token)).expect("both reused slots must remain in FIFO order");
            assert_eq!(retired.0, expected);
        }
        assert_eq!(lane.view(&token).len(), 0);
    });
}

#[test]
fn restores_front() {
    region::Token::scope(|mut token| {
        let plan = fair::Plan::new(2, 1).expect("two slots fit one lane");
        let lanes = fair::Lanes::<u64>::try_new(plan, &token).expect("queue allocation");
        let lane = lanes.bind(0).expect("configured lane");
        let full = Cause::Exhausted {
            axis: Axis::at::<0>(),
            required: 1,
            available: 0,
        };
        for value in [11, 13] {
            lane.reserve()
                .expect("each slot has credit")
                .push_back(&mut token, value);
        }
        assert!(matches!(lane.reserve(), Err(cause) if cause == full));

        let (value, detached) = lane.take_front(&mut token).expect("retained head");
        assert_eq!(value, 11);
        assert_eq!(lane.view(&token).front(), Some(&13));
        assert_eq!(lane.view(&token).len(), 1);
        assert!(matches!(lane.reserve(), Err(cause) if cause == full));
        detached.push_front(&mut token, value);
        assert_eq!(lane.view(&token).len(), 2);
        assert!(matches!(lane.reserve(), Err(cause) if cause == full));

        for expected in [11, 13] {
            let retired = lane.take_front(&mut token).expect("restored FIFO order");
            assert_eq!(retired.0, expected);
        }
        assert!(lane.take_front(&mut token).is_none());

        let second;
        {
            let first = lane.reserve();
            assert!(first.is_ok(), "first refunded credit");
            second = lane.reserve();
            assert!(second.is_ok(), "second refunded credit");
            assert!(matches!(lane.reserve(), Err(cause) if cause == full));
        }
        let first = lane.reserve();
        assert!(first.is_ok(), "one cancellation refunds one credit");
        assert!(matches!(lane.reserve(), Err(cause) if cause == full));
    });
}

#[test]
fn protects_idle_lanes() {
    for first in 0..3 {
        region::Token::scope(|mut token| {
            let plan = (fair::Plan::new(5, 3)).expect("five slots must fit three lanes");
            let lanes = (fair::Lanes::<u64>::try_new(plan, &token))
                .expect("the three-lane queue must allocate");
            let (a, b, c) =
                match [first, (first + 1) % 3, (first + 2) % 3].map(|index| lanes.bind(index)) {
                    [Some(a), Some(b), Some(c)] => Some((a, b, c)),
                    _ => None,
                }
                .expect("all three configured lanes must exist");
            assert_eq!(
                [a.index(), b.index(), c.index()],
                [first, (first + 1) % 3, (first + 2) % 3]
            );
            for value in [21, 22, 23] {
                let permit = a
                    .reserve()
                    .expect("credit demand must fit its original lane");

                permit.push_back(&mut token, value);
            }
            let full = Cause::Exhausted {
                axis: Axis::at::<0>(),
                required: 1,
                available: 0,
            };
            assert!(matches!(a.reserve(), Err(cause) if cause == full));
            {
                let second = b
                    .reserve()
                    .expect("credit demand must fit its original lane");
                let third = c.reserve();
                assert!(third.is_ok(), "credit demand must fit its original lane");
                second.push_back(&mut token, 31);
            }
            assert!(matches!(a.reserve(), Err(cause) if cause == full));

            {
                let detached =
                    (a.take_front(&mut token)).expect("the first occupied slot must detach");
                assert_eq!(detached.0, 21);
                assert!(matches!(a.reserve(), Err(cause) if cause == full));
                let permit = (c.reserve()).expect("credit demand must fit its original lane");

                permit.push_back(&mut token, 41);
                assert!(matches!(c.reserve(), Err(cause) if cause == full));
            }
            let shared = (c.reserve()).expect("credit demand must fit its original lane");

            shared.push_back(&mut token, 42);
            for (lane, expected) in [(a, 22), (c, 41), (b, 31), (c, 42), (a, 23)] {
                let retired = (lane.take_front(&mut token))
                    .expect("interleaved retirement must retain every queued value");
                assert_eq!(retired.0, expected);
            }
            for lane in [a, b, c] {
                assert_eq!(lane.view(&token).len(), 0);
                let permits = [lane.reserve(), lane.reserve(), lane.reserve()];
                assert!(permits.iter().all(Result::is_ok), "three refunded credits");
                assert!(matches!(lane.reserve(), Err(cause) if cause == full));
            }
        });
    }
}

#[test]
fn fair_keeps_allocation() {
    region::Token::scope(|mut token| {
        let plan = (fair::Plan::new(4, 1)).expect("four slots must fit one lane");
        let (created, alloc) = Tracker::measure(|| fair::Lanes::<u64, u64>::try_new(plan, &token));
        assert_eq!(alloc, [1, 96, 0, 0]);
        let freed = {
            let lanes = (created).expect("the fair queue must allocate once");
            assert_eq!(lanes.capacity().get(), 1);
            // SAFETY: this owner retains the one initialized lane admitted by its plan.
            let source = unsafe { depot::raw::Proof::new((&lanes, 0)) };
            let lane = fair::Binding::from_source(source);
            let ((), hot) = Tracker::measure(|| {
                let first = (lane.reserve()).expect("credit demand must fit its original lane");

                let value = first.state_mut(&mut token);
                let second = lane.reserve();
                second.as_ref().expect("the second reservation must fit");
                *value += 1;
                assert_eq!(*value, 1);
                first.push_back(&mut token, 11);
                let (value, permit) = (lane.take_front(&mut token))
                    .expect("publication must retain the reserved slot");
                permit.push_back(&mut token, value + 2);
                let retired = (lane.take_front(&mut token))
                    .expect("the detached reservation must publish without allocation");
                assert_eq!(retired.0, 13);
            });
            assert_eq!(hot, [0; 4]);
            Tracker::begin()
        };
        let freed = freed.finish();
        assert_eq!((freed.allocations, freed.allocated_bytes), (0, 0));
        assert_eq!((freed.deallocations, freed.deallocated_bytes), (1, 96));
    });
}

#[test]
fn fair_preserves_order() {
    region::Token::scope(|mut token| {
        let plan = (fair::Plan::new(4, 2)).expect("four slots must fit two lanes");
        let lanes =
            (fair::Lanes::<u64>::try_new(plan, &token)).expect("the fair queues must allocate");
        let left = lanes.bind(0).expect("first configured lane");
        let right = lanes.bind(1).expect("second configured lane");
        let ((), activity) = Tracker::measure(|| {
            for value in [11, 22, 33] {
                let permit = (left.reserve()).expect("credit demand must fit its original lane");

                permit.push_back(&mut token, value);
            }
            let permit = (right.reserve()).expect("credit demand must fit its original lane");

            permit.push_back(&mut token, 44);
            let (value, permit) =
                (left.take_front(&mut token)).expect("the original head must remain queued");
            assert_eq!(value, 11);
            assert!(left.reserve().is_err());
            assert!(right.reserve().is_err());
            permit.push_front(&mut token, value);
            assert!(left.reserve().is_err());
            assert!(right.reserve().is_err());
            for expected in [11, 22, 33] {
                assert!(
                    matches!(left.take_front(&mut token), Some((value, _)) if value == expected)
                );
            }
            assert_eq!(left.view(&token).len(), 0);
            assert!(matches!(right.take_front(&mut token), Some((44, _))));
            let refunds = [
                left.reserve(),
                left.reserve(),
                left.reserve(),
                right.reserve(),
            ];
            assert!(refunds.iter().all(Result::is_ok));
            assert!(left.reserve().is_err());
            assert!(right.reserve().is_err());
        });
        assert_eq!(activity, [0; 4]);
    });
}
