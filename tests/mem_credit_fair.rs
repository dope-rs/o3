pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, num::NonZero};

use common::{
    alloc::system::Tracker,
    mem::{Attach, Bytes, Lanes, Partition, Reentry},
};
use o3::mem::{self, credit::fair, raw::Source as _};

#[test]
fn computes_waiting_demand() {
    let balance = mem::raw::Balance::new([1, u32::MAX, 1], [0; 3]);
    assert_eq!(
        balance.demand([u32::MAX, u32::MAX, 2], [u32::MAX, u32::MAX, 4]),
        [1, u32::MAX, 3],
    );
}

#[test]
fn protects_and_shares() {
    let accounts = Lanes::try_new([4], [[2], [2]]).expect("fixture lane capacity");
    let first = accounts.get(0).expect("first lane");
    let second = accounts.get(1).expect("second lane");
    let part = mem::raw::Grant::from(
        first
            .try_acquire([3])
            .expect("credit acquisition must retain its exact amount"),
    );
    let rest = mem::raw::Grant::from(
        first
            .try_acquire([3])
            .expect("credit acquisition must retain its exact amount"),
    );
    let Err(failure) = first.try_acquire([1]) else {
        panic!("credit request exceeds the available amount");
    };
    assert!(
        failure.into_parts()
            == (
                [1],
                fair::Cause::Exhausted {
                    axis: fair::Axis::at::<0>(),
                    required: 1,
                    available: 0,
                },
            )
    );
    let two = mem::raw::Grant::from(
        second
            .try_acquire([2])
            .expect("credit acquisition must retain its exact amount"),
    );
    assert_eq!([first.held()[0] + second.held()[0]], [8]);

    part.release();
    assert_eq!(accounts.shared(), [3]);
    let one = mem::raw::Grant::from(
        second
            .try_acquire([1])
            .expect("credit acquisition must retain its exact amount"),
    );
    assert_eq!(first.held(), [3]);
    assert_eq!(second.held(), [3]);

    let Err(failure) = first.try_acquire([9]) else {
        panic!("credit request exceeds the available amount");
    };
    assert!(
        failure.into_parts()
            == (
                [9],
                fair::Cause::Exhausted {
                    axis: fair::Axis::at::<0>(),
                    required: 9,
                    available: 2,
                },
            )
    );
    rest.release();
    two.release();
    one.release();
}

#[test]
fn credits_after_reentry() {
    let accounts = Lanes::try_new([3], [[0], [0]]).expect("shared credit");
    let lane = accounts.get(0).expect("first lane");
    let other = accounts.get(1).expect("second lane");
    let acquire = Cell::new(Some(2));
    let nested = Cell::new(None);
    let failure = Cell::new(None);
    let source = Reentry {
        lane,
        other,
        acquire: &acquire,
        nested: &nested,
        failure: &failure,
    };
    let Err(rejected) = mem::raw::Charge::try_new(source, [2]) else {
        panic!("nested acquisition leaves one shared unit");
    };
    assert_eq!(
        rejected.into_parts(),
        (
            [2],
            fair::Cause::Exhausted {
                axis: fair::Axis::at::<0>(),
                required: 2,
                available: 1,
            }
        )
    );
    assert_eq!(
        (lane.held(), other.held(), accounts.shared()),
        ([0], [2], [1])
    );
    drop(nested.take());

    let charge = mem::raw::Charge::try_new(source, [2]).expect("refunded credit fits");
    acquire.set(Some(1));
    drop(charge);
    assert_eq!(
        (lane.held(), other.held(), accounts.shared()),
        ([0], [1], [2])
    );
    drop(nested.take());
    assert_eq!(
        (lane.held(), other.held(), accounts.shared()),
        ([0], [0], [3])
    );
    assert!(failure.get().is_none());
}

#[test]
fn isolates_pool_accounts() {
    let first = Lanes::try_new([10], [[0]]).expect("fixture lane capacity");
    let second = Lanes::try_new([10], [[0]]).expect("fixture lane capacity");
    let lane0 = first.get(0).expect("first account");
    let lane1 = second.get(0).expect("second account");

    let grant0 = mem::raw::Grant::from(
        lane0
            .try_acquire([5])
            .expect("credit acquisition must retain its exact amount"),
    );
    let grant1 = mem::raw::Grant::from(
        lane1
            .try_acquire([10])
            .expect("credit acquisition must retain its exact amount"),
    );
    grant0.release();
    assert_eq!(first.shared(), [10]);
    assert_eq!(second.shared(), [0]);
    assert_eq!(lane0.held(), [0]);
    assert_eq!(lane1.held(), [10]);
    grant1.release();
    assert_eq!(second.shared(), [10]);
    assert_eq!(lane1.held(), [0]);
}

#[test]
fn extends_full_width() {
    assert_eq!(std::mem::size_of::<mem::raw::Pool>(), 16);
    assert_eq!(std::mem::size_of::<mem::raw::Pool<2>>(), 32);
    assert_eq!(std::mem::size_of::<mem::raw::State>(), 16);
    assert_eq!(std::mem::size_of::<mem::raw::State<2>>(), 32);
    assert_eq!(std::mem::size_of::<mem::raw::Lane<'_, '_>>(), 16);
    assert_eq!(std::mem::size_of::<mem::raw::Lane<'_, '_, 2>>(), 16);
    let pool = mem::raw::Pool::<1>::new([usize::MAX]);
    let state = mem::raw::State::new([0]);
    // SAFETY: this state stays uniquely bound to the same pool until every grant returns.
    let lane = unsafe { pool.lane(&state) };
    let mut held = mem::raw::Grant::empty(lane);
    held.try_extend([usize::MAX - 1])
        .expect("the initial amount fits")
        .merge();
    {
        let pending = held.try_extend([1]);
        assert!(pending.is_ok(), "the final unit fits");
        assert_eq!(lane.held(), [usize::MAX]);
    }
    assert_eq!(
        (held.amount(), lane.held(), pool.shared()),
        ([usize::MAX - 1], [usize::MAX - 1], [1])
    );
    held.try_extend([1])
        .expect("the refunded unit can be readmitted")
        .merge();
    let Err(error) = held.try_extend([1]) else {
        panic!("one more unit must overflow");
    };
    let (_, cause) = error.into_parts();
    assert_eq!(
        cause,
        fair::Cause::Overflow {
            axis: fair::Axis::at::<0>(),
            held: usize::MAX,
            requested: 1
        }
    );
    assert_eq!(held.amount(), [usize::MAX]);
    held.release();
    assert_eq!((lane.held(), pool.shared()), ([0], [usize::MAX]));
}

#[test]
fn rejects_late_axis() {
    let accounts = Lanes::try_new([4, 40], [[2, 20]]).expect("fixture lane capacity");
    let lane = accounts.get(0).expect("first lane");
    let ((), alloc) = Tracker::usage(|| {
        assert!(!lane.can_acquire([6, 61]));
        let Err(error) = lane.try_acquire([6, 61]) else {
            panic!("second axis exhausted");
        };
        assert_eq!(
            error.into_parts(),
            (
                [6, 61],
                fair::Cause::Exhausted {
                    axis: fair::Axis::at::<1>(),
                    required: 41,
                    available: 40,
                }
            )
        );
        assert_eq!(lane.held(), [0, 0]);
        assert_eq!(accounts.shared(), [4, 40]);
        {
            let charge = lane.try_acquire([6, 60]);
            assert!(charge.is_ok(), "all credit remains");
            assert_eq!(lane.held(), [6, 60]);
            assert_eq!(accounts.shared(), [0, 0]);
        }
        assert_eq!(lane.held(), [0, 0]);
        assert_eq!(accounts.shared(), [4, 40]);
        let retry = lane.try_acquire([6, 60]);
        assert!(retry.is_ok(), "refunded credit can be readmitted");
    });
    assert_eq!(alloc, (0, 0));
}

#[test]
fn overflows_late_axis() {
    let accounts = Lanes::try_new([u32::MAX; 2], [[0; 2]]).expect("fixture lane capacity");
    let lane = accounts.get(0).expect("configured lane index");
    let ((), alloc) = Tracker::usage(|| {
        {
            let charge = lane.try_acquire([u32::MAX; 2]);
            assert!(charge.is_ok(), "full width fits");
            assert!(!lane.can_acquire([0, 1]));
            let Err(error) = lane.try_acquire([0, 1]) else {
                panic!("second axis overflows");
            };
            assert_eq!(
                error.into_parts(),
                (
                    [0, 1],
                    fair::Cause::Overflow {
                        axis: fair::Axis::at::<1>(),
                        held: u32::MAX,
                        requested: 1,
                    }
                )
            );
            assert_eq!(lane.held(), [u32::MAX; 2]);
            assert_eq!(accounts.shared(), [0, 0]);
        }
        assert_eq!(lane.held(), [0, 0]);
        assert_eq!(accounts.shared(), [u32::MAX; 2]);
    });
    assert_eq!(alloc, (0, 0));
}

#[test]
fn bound_partition() {
    let accounts = Lanes::try_new([4, 100], [[0, 0]]).expect("fixture lane capacity");
    let lane = accounts.get(0).expect("configured lane index");
    let bytes = NonZero::<u32>::MIN.saturating_add(9);
    let ((), activity) = Tracker::measure(|| {
        let mut grant =
            mem::raw::Grant::from(lane.try_acquire([3, 90]).expect("three entries fit"));
        // SAFETY: [2, 60] leaves exactly one entry and thirty resident units.
        let mut part = unsafe { grant.bind_mut(Partition([2, 60])) }.split(bytes);
        assert_eq!(grant.amount(), [2, 60]);
        assert_eq!(lane.held(), [3, 90]);
        part.update(Bytes(NonZero::<u32>::MIN));
        assert_eq!(part.amount().bytes, NonZero::<u32>::MIN);
        let part = part.into_array();
        assert_eq!(part.amount(), [1, 30]);
        assert_eq!(lane.held(), [3, 90]);
        part.release();
        assert_eq!(lane.held(), [2, 60]);
        grant.release();
        assert_eq!(lane.held(), [0, 0]);
        assert_eq!(accounts.shared(), [4, 100]);
    });
    assert_eq!(activity, [0; 4]);
}

#[test]
fn owned_partition() {
    let accounts = Lanes::try_new([2, 60], [[0, 0]]).expect("fixture lane capacity");
    let lane = accounts.get(0).expect("configured lane index");
    let bytes = NonZero::<u32>::MIN.saturating_add(7);
    let ((), activity) = Tracker::measure(|| {
        let source = mem::raw::Grant::from(lane.try_acquire([2, 60]).expect("two entries fit"));
        // SAFETY: this exact source splits into [1, 40] and [1, 20].
        let (rest, part) = unsafe { source.bind(Partition([1, 40])) }.split(bytes);
        // SAFETY: the retained remainder contains exactly one entry.
        let rest = unsafe { rest.bind(Attach(bytes)) }.map();
        assert_eq!(lane.held(), [2, 60]);
        part.release();
        assert_eq!(lane.held(), [1, 40]);
        rest.release();
        assert_eq!(lane.held(), [0, 0]);
        assert_eq!(accounts.shared(), [2, 60]);
    });
    assert_eq!(activity, [0; 4]);
}
