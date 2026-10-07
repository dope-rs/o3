pub mod common;

use std::{cell, mem, num};

use common::alloc::system::Tracker;
use o3::depot::queue::round::{self, phase};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn preserves_rotation() {
    let created = round::Lanes::<u64, 2>::try_new(3, num::NonZero::<usize>::MIN.saturating_add(1));
    let queue = created.expect("required fixture outcome");
    queue
        .try_slot(0)
        .expect("resident")
        .try_lane(0)
        .expect("destination")
        .push();
    queue
        .try_slot(1)
        .expect("resident")
        .try_lane(0)
        .expect("destination")
        .push();
    queue
        .try_slot(2)
        .expect("resident")
        .try_lane(1)
        .expect("destination")
        .push();
    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Present
    );
    assert_eq!(
        round::Lane::try_new(&queue, 0)
            .expect("lane")
            .front()
            .map(|front| front.index()),
        Some(0)
    );
    queue
        .try_slot(0)
        .expect("resident")
        .try_lane(0)
        .expect("destination")
        .push();
    assert_eq!(
        round::Lane::try_new(&queue, 0)
            .expect("lane")
            .front()
            .map(|front| front.index()),
        Some(1)
    );
    assert!(
        queue
            .try_slot(1)
            .expect("resident")
            .try_lane(1)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    queue.try_slot(0).expect("resident").clear();
    assert!(queue.group::<0>().active().is_none());
    assert_eq!(
        round::Lane::try_new(&queue, 1)
            .expect("lane")
            .front()
            .map(|front| front.index()),
        Some(2)
    );
    let stale = round::Lane::try_new(&queue, 1).expect("lane").front();
    assert!(stale.is_some());
    let front = round::Lane::try_new(&queue, 1)
        .expect("lane")
        .rotate()
        .expect("rotation retains the next front");
    assert!(stale.and_then(|front| front.get()).is_none());
    assert_eq!(front.index(), 1);
    assert!(front.pop().is_some());
    assert_eq!(
        round::Lane::try_new(&queue, 1)
            .expect("lane")
            .front()
            .map(|front| front.index()),
        Some(2)
    );
    queue.try_slot(2).expect("resident").clear();
    assert!(
        round::Lane::try_new(&queue, 1)
            .expect("lane")
            .rotate()
            .is_none()
    );
    assert!(queue.group::<1>().active().is_none());
}

#[test]
fn proofs_reject_stale() {
    let queue = round::Lanes::<cell::Cell<usize>, 2>::try_new(
        4,
        num::NonZero::<usize>::MIN.saturating_add(2),
    );
    let queue = (queue).expect("the stale-proof lanes must allocate");
    for index in 0..4 {
        queue.try_slot(index).expect("resident").get().set(index);
    }
    let lane0 = round::Lane::try_new(&queue, 0).expect("lane");
    let group0 = queue.group::<0>();

    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(
        queue
            .try_slot(1)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(
        queue
            .try_slot(2)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(
        queue
            .try_slot(3)
            .expect("resident")
            .try_lane(2)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );

    let front = (lane0.front()).expect("lane front must exist before invalidation");
    assert_eq!(lane0.pop_front().map(|slot| slot.get()), Some(0));
    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(front.get().is_none());
    assert!(front.pop_if(|_| true).is_none());

    let turn = (group0.active()).expect("group turn must exist before invalidation");
    assert_eq!(turn.lane(), 0);
    assert_eq!(
        group0.next().map(|(lane, slot)| (lane, slot.get())),
        Some((0, 1))
    );
    assert!(turn.get().is_none());
    assert!(turn.pop_if(|_| true).is_none());
    assert_eq!(group0.active().map(|next| next.lane()), Some(2));
}

#[test]
fn retains_fixed_lanes() {
    use o3::{base::num::bounded, depot::queue::round};

    let (queue, cold) = Tracker::measure(|| round::Lanes::<cell::Cell<usize>, 2, 2>::try_fixed(2));
    let queue = queue.expect("fixed owner");
    assert_eq!(cold[0], 1);
    let first = queue.try_slot(0).expect("first resident");
    let second = queue.try_slot(1).expect("second resident");
    second.get().set(1);
    let placed = first.lane(bounded::U32::<0, 1>::exact::<1>());
    let peer = second.lane(bounded::U32::<0, 1>::exact::<1>());
    let lane = queue.lane(bounded::U32::<0, 1>::exact::<1>());
    let ((), hot) = Tracker::measure(|| {
        for _ in 0..128 {
            assert!(placed.set() == round::Insertion::Inserted);
            peer.push();
            placed.push();
            assert_eq!(lane.pop_front().map(|entry| entry.get()), Some(1));
            assert_eq!(lane.pop_front().map(|entry| entry.get()), Some(0));
            assert!(lane.pop_front().is_none());
        }
        assert_eq!(
            queue.try_slot(2).err().expect("range"),
            round::Bounds::Slot {
                index: 2,
                capacity: 2
            }
        );
    });
    assert_eq!(hot, [0; 4]);
}

#[test]
fn stripes_share_allocation() {
    const CAPACITY: usize = 257;
    const LANES: usize = 17;
    const GROUPS: usize = 5;
    const ENTRY_BYTES: usize = 32;
    const LANE_BYTES: usize = 16;
    const OWNER_BYTES: usize = 4;
    const ALIGN_PADDING: usize = 4;

    let (queue, alloc) = Tracker::measure(|| {
        round::Lanes::<[cell::Cell<u64>; 3], GROUPS>::try_new(
            CAPACITY,
            num::NonZero::<usize>::MIN.saturating_add(LANES - 1),
        )
    });
    assert_eq!(
        mem::size_of::<round::Lanes<[cell::Cell<u64>; 3], GROUPS>>(),
        6 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            round::Snapshot<&'static round::Lanes<[cell::Cell<u64>; 3], GROUPS>, round::Front>,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            round::Snapshot<&'static round::Lanes<[cell::Cell<u64>; 3], GROUPS>, round::Turn>,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            round::Snapshot<
                &'static mut round::Lanes<[cell::Cell<u64>; 3], GROUPS>,
                round::Idle<false>,
            >,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<
                round::Snapshot<
                    &'static mut round::Lanes<[cell::Cell<u64>; 3], GROUPS>,
                    round::Idle<false>,
                >,
            >,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        alloc,
        [
            1,
            CAPACITY * (ENTRY_BYTES + OWNER_BYTES) + LANES * LANE_BYTES + ALIGN_PADDING,
            0,
            0,
        ]
    );
    let queue = (queue).expect("the striped shared allocation must succeed");
    for index in 0..CAPACITY {
        queue.try_slot(index).expect("resident").get()[0].set(index as u64);
    }
    let places = [(0, 0), (1, 5), (2, 1), (3, 6), (4, 0)].map(|(index, lane)| {
        queue
            .try_slot(index)
            .expect("resident")
            .try_lane(lane)
            .expect("destination")
    });
    let first = places[0].slot().get();
    let lane5 = round::Lane::try_new(&queue, 5).expect("lane");
    let group0 = queue.group::<0>();
    let group1 = queue.group::<1>();

    let ((), alloc) = Tracker::measure(|| {
        use round::Insertion::{Inserted, Present};

        for (index, expected) in [
            (0, Inserted),
            (0, Present),
            (1, Inserted),
            (2, Inserted),
            (3, Inserted),
            (4, Inserted),
            (4, Present),
        ] {
            assert!(places[index].set() == expected);
        }
        assert_eq!(group0.active().map(|turn| turn.lane()), Some(0));
        assert_eq!(group1.active().map(|turn| turn.lane()), Some(1));

        assert_eq!(
            group0.next().map(|(_, slot)| slot[0].get() as usize),
            Some(0)
        );
        assert_eq!(group0.active().map(|turn| turn.lane()), Some(5));
        assert_eq!(
            lane5.pop_front().map(|slot| slot[0].get() as usize),
            Some(1)
        );
        assert_eq!(group0.active().map(|turn| turn.lane()), Some(0));

        assert_eq!(places[4].slot().clear()[0].get() as usize, 4);
        assert_eq!(group0.active().map(|turn| turn.lane()), None);
        assert_eq!(
            group1
                .next()
                .map(|(lane, slot)| (lane, slot[0].get() as usize)),
            Some((1, 2))
        );
        assert_eq!(
            group1.next().map(|(_, slot)| slot[0].get() as usize),
            Some(3)
        );
        assert_eq!(group1.active().map(|turn| turn.lane()), None);
        assert!(std::ptr::eq(first, places[0].slot().get()));
    });
    assert_eq!(alloc, [0, 0, 0, 0]);

    let ((), alloc) = Tracker::measure(move || drop(queue));
    assert_eq!(
        alloc,
        [
            0,
            0,
            1,
            CAPACITY * (ENTRY_BYTES + OWNER_BYTES) + LANES * LANE_BYTES + ALIGN_PADDING,
        ]
    );
}

#[test]
fn dequeue_advances_lane() {
    let queue = round::Lanes::<usize, 1>::try_new(2, num::NonZero::<usize>::MIN.saturating_add(1));
    let mut queue = (queue).expect("the two-lane dequeue fixture must allocate");
    assert!(
        queue
            .try_slot(0)
            .expect("resident")
            .try_lane(0)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert!(
        queue
            .try_slot(1)
            .expect("resident")
            .try_lane(1)
            .expect("destination")
            .set()
            == round::Insertion::Inserted
    );
    assert_eq!(queue.group::<0>().active().map(|turn| turn.lane()), Some(0));
    let mut selected = round::Lane::try_new(&mut queue, 0).expect("lane");
    let dequeued = selected.dequeue();
    assert_eq!(*dequeued.expect("queued resident").get(), 0);
    assert_eq!(queue.group::<0>().active().map(|turn| turn.lane()), Some(1));
    let mut selected = round::Lane::try_new(&mut queue, 1).expect("lane");
    let dequeued = selected.dequeue();
    assert_eq!(*dequeued.expect("queued resident").get(), 0);
}

#[test]
fn dequeue_middle_is_fair() {
    for capacity in [3, 4] {
        let mut queue = round::Lanes::<cell::Cell<usize>, 1>::try_new(
            capacity,
            num::NonZero::<usize>::MIN.saturating_add(2),
        )
        .expect("three lanes");
        for index in 0..capacity {
            let slot = queue.try_slot(index).expect("resident");
            slot.get().set(index);
            slot.try_lane(if index == 3 { 1 } else { index })
                .expect("destination")
                .push();
        }

        let mut selected = round::Lane::try_new(&mut queue, 1).expect("middle lane");
        assert_eq!(selected.dequeue().expect("resident").get().get(), 1);
        let group = queue.group::<0>();
        assert_eq!(
            [(); 4].map(|_| group.next().map(|(lane, value)| (lane, value.get()))),
            [
                Some((0, 0)),
                Some((2, 2)),
                (capacity == 4).then_some((1, 3)),
                None,
            ]
        );
    }
}

#[test]
fn retains_head() {
    use o3::depot::raw::{Admit as _, Proof};

    let queue = round::Lanes::<cell::Cell<usize>, 1>::try_new(
        3,
        num::NonZero::<usize>::MIN.saturating_add(1),
    )
    .expect("lanes");
    let lane = round::Lane::try_new(&queue, 0).expect("lane");
    // SAFETY: this test owns the empty topology through this admission.
    let empty = unsafe { Proof::<_, phase::Ready>::new(lane) };
    assert!(empty.admit().is_none());
    for (index, lane) in [(0, 0), (1, 0), (2, 1)] {
        let slot = queue.try_slot(index).expect("resident");
        slot.get().set(index);
        slot.try_lane(lane).expect("destination").push();
    }
    let group = queue.group::<0>();
    // SAFETY: no alias mutates membership until this admitted head is consumed.
    let head = unsafe { Proof::<_, phase::Ready>::new(lane) }
        .admit()
        .expect("queued head");
    assert_eq!((head.index(), head.get().get()), (0, 0));
    let head = head.rotate();
    assert_eq!((head.index(), head.get().get()), (1, 1));
    assert_eq!(group.active().map(|turn| turn.lane()), Some(1));
    assert_eq!(head.pop().get(), 1);
    assert_eq!(
        group.next().map(|(lane, value)| (lane, value.get())),
        Some((1, 2))
    );
    // SAFETY: no alias mutates membership until this admitted head is consumed.
    let head = unsafe { Proof::<_, phase::Ready>::new(lane) }
        .admit()
        .expect("single head")
        .rotate();
    assert_eq!(head.index(), 0);
    assert_eq!(head.pop().get(), 0);
    assert!(group.active().is_none());
}
