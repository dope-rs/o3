pub mod common;

use std::{mem, num::NonZero};

use common::alloc::system::Tracker;
use o3::depot::queue::resident::{self, Drain, Fixed, Take, fifo};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn retains_owner_and_slot() {
    let owner = fifo::Fifo::try_new(1, Fixed::<2>).expect("resident owner");
    let other = fifo::Fifo::try_new(1, Fixed::<2>).expect("other owner");
    let mut active = fifo::Registration::new();
    let mut spare = fifo::Registration::new();

    owner
        .first()
        .try_keep(&mut active, 11)
        .expect("first value");
    assert_eq!(
        other.lane::<1>().try_keep(&mut active, 22),
        Err(fifo::Reject::Active { index: 1 }),
    );
    owner
        .lane::<1>()
        .try_keep(&mut active, 33)
        .expect("move lane");
    assert!(owner.lane::<0>().take_one() == Take::Empty);
    assert!(owner.lane::<1>().take_one() == Take::Item(33));
    assert_eq!(
        owner.lane::<0>().try_keep(&mut spare, 44),
        Err(fifo::Reject::Full { index: 0 }),
    );
    owner
        .lane::<0>()
        .try_keep(&mut active, 55)
        .expect("rearm slot");
    drop(active);
    owner
        .lane::<1>()
        .try_keep(&mut spare, 66)
        .expect("returned slot");
    assert!(owner.take_next() == Some(66));
    drop(spare);
    let mut next = fifo::Registration::new();
    owner
        .lane::<0>()
        .try_keep(&mut next, 77)
        .expect("empty slot returned");
    assert!(owner.take_next() == Some(77));
    assert!(owner.take_next().is_none());
}

#[test]
fn scope_retains_admission() {
    fifo::Fifo::try_with(1, NonZero::<usize>::MIN, 91, |source, fifo| {
        let index = fifo.try_index(0).expect("only lane");
        assert!(fifo.try_index(1).is_err());
        let mut slot = fifo::Registration::new();
        fifo.lane(index)
            .try_keep(&mut slot, *source)
            .expect("scoped value");
        assert!(fifo.first().take_one() == Take::Item(*source));
    })
    .expect("scoped owner");
}

#[test]
fn compacts_scoped_indices() {
    let max = usize::from(u16::MAX);
    let lanes = NonZero::new(max + 2).expect("lane extent");
    fifo::Fifo::<()>::try_with(0, lanes, (), |_, fifo| {
        for index in [0, max + 1] {
            let index = fifo.try_index(index).expect("admitted lane");
            assert!(fifo::Compact::try_from(index).is_err());
        }
        let [first, upper, last] = [1, 3, max].map(|index| {
            let index = fifo.try_index(index).expect("admitted lane");
            fifo::Compact::try_from(index).expect("positive compact lane")
        });
        assert_eq!(first.get(), NonZero::<u16>::MIN);
        assert_eq!(last.get(), NonZero::<u16>::MAX);
        for compact in [first, upper, last] {
            let index = compact.into_index();
            assert_eq!(fifo::Compact::try_from(index), Ok(compact));
            assert!(fifo.lane(index).take_one() == Take::Empty);
        }
        assert_eq!(upper.after(None), Some(first));
        assert_eq!(
            upper.after(Some(first)).map(|index| index.get()),
            NonZero::new(2)
        );
        assert_eq!(upper.after(Some(upper)), None);
        assert_eq!(upper.after(Some(last)), None);
        assert_eq!(last.after(Some(last)), None);
    })
    .expect("scoped lane extent");
}

#[test]
fn drains_and_reuses() {
    let queue = fifo::Fifo::try_new(3, NonZero::<usize>::MIN.saturating_add(1)).expect("lanes");
    let ((), hot) = Tracker::measure(|| {
        let lane0 = queue.first();
        let lane1 = queue.try_lane(1).expect("second lane");
        let mut first = fifo::Registration::new();
        let mut second = fifo::Registration::new();
        let mut third = fifo::Registration::new();
        lane0.try_keep(&mut first, 10).expect("first");
        lane0.try_keep(&mut second, 11).expect("second");
        lane1.try_keep(&mut third, 20).expect("third");
        for value in [10, 20, 11] {
            assert_eq!(queue.take_next(), Some(value));
        }
        assert_eq!(queue.take_next(), None);
        lane0.try_keep(&mut first, 30).expect("reuse first");
        lane0.try_keep(&mut second, 31).expect("reuse second");
        assert_eq!(queue.take_next(), Some(30));
        assert_eq!(queue.take_next(), Some(31));
        lane0.try_keep(&mut first, 40).expect("drain first");
        lane0.try_keep(&mut second, 41).expect("drain second");
        lane1.try_keep(&mut third, 50).expect("drain third");
        queue.begin_drain();
        assert_eq!(
            lane0.try_keep(&mut first, 99),
            Err(fifo::Reject::Draining { index: 0 })
        );
        assert!(queue.take_next().is_none());
        for value in [40, 41, 50] {
            assert!(queue.drain_next() == Drain::Item(value));
        }
        assert!(queue.drain_next() == Drain::Done);
        assert!(!queue.is_draining());
        lane0.try_keep(&mut first, 60).expect("drain ended");
    });
    assert_eq!(hot, [0; 4]);
    assert!(!queue.has_waiters());
    let mut next = fifo::Registration::new();
    queue
        .try_lane(0)
        .expect("first lane")
        .try_keep(&mut next, 70)
        .expect("returned slot");
    assert_eq!(queue.take_next(), Some(70));
}

#[test]
fn storage_is_exact() {
    let word = mem::size_of::<usize>();
    assert_eq!(
        [
            mem::size_of::<fifo::Fifo<usize>>(),
            mem::size_of::<Option<fifo::Fifo<usize>>>(),
            mem::size_of::<fifo::Fifo<usize, Fixed<1>>>(),
            mem::size_of::<Option<fifo::Fifo<usize, Fixed<1>>>>(),
        ],
        [5 * word; 4]
    );
    assert_eq!(
        mem::size_of::<fifo::Registration<'static, usize, Fixed<1>>>(),
        2 * word
    );
    assert_eq!(
        mem::size_of::<Option<fifo::Registration<'static, usize>>>(),
        3 * word
    );
    for (capacity, lanes, bytes) in [(0, 3, 48), (1, 3, 64), (3, 2, 80), (5, 1, 96)] {
        let lanes = NonZero::new(lanes).expect("positive extent");
        let (result, cold) = Tracker::measure(|| fifo::Fifo::<usize>::try_new(capacity, lanes));
        assert_eq!(cold, [1, bytes, 0, 0]);
        let queue = result.expect("resident allocation");
        assert!(queue.first().take_one() == Take::Empty);
        for index in 0..lanes.get() {
            assert!(queue.try_lane(index).expect("allocated lane").take_one() == Take::Empty);
        }
        for index in [lanes.get(), u32::MAX as usize, usize::MAX] {
            let error = queue.try_lane(index).err().expect("outside lane domain");
            assert_eq!(
                (error.index, error.len.get() as usize),
                (index, lanes.get())
            );
        }
        let ((), freed) = Tracker::measure(move || assert!(!queue.has_waiters()));
        assert_eq!(freed, [0, 0, 1, bytes]);
    }
    let (result, cold) = Tracker::measure(|| fifo::Fifo::<usize, _>::try_new(5, Fixed::<1>));
    assert_eq!(cold, [1, 96, 0, 0]);
    let queue = result.expect("fixed allocation");
    let ((), freed) = Tracker::measure(move || assert!(!queue.has_waiters()));
    assert_eq!(freed, [0, 0, 1, 96]);
    let (result, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || fifo::Fifo::<usize>::try_new(3, NonZero::<usize>::MIN))
    });
    assert!(matches!(result, Err(resident::Error::Exhausted(_))));
    assert_eq!(activity, [0; 4]);
}
