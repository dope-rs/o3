pub mod common;

use std::mem;

use common::{
    alloc::system::Tracker,
    depot::set::{Key, claim},
};
use o3::depot::{
    capacity,
    index::{
        self, Domain, Ordinal,
        bitmap::{self, Fill},
        set::{self, Disjoint, Entry, Id, State},
    },
    raw,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn projects_retained_rows() {
    let domain = Domain::<(), u16>::try_new(65).expect("domain");
    let source = State::<(), u16, 2>::try_new(domain, [Fill::Empty, Fill::Full]).expect("source");
    let other = State::<(), u16, 2>::try_new(domain, [Fill::Empty; 2]).expect("other");
    for ordinal in [0, 64] {
        let key = Ordinal::new(ordinal);
        // SAFETY: both admitted ordinals belong to this exact initialized 65-row source.
        let proof = unsafe { raw::Proof::<_, index::Initialized<()>>::new((&source, key)) };
        let row = proof.into();
        assert_eq!(row.index().raw(), ordinal);
        row.set(Id::at::<0>());
        row.clear(Id::at::<1>());
        assert!(source.contains(Id::at::<0>(), key));
        assert!(!source.contains(Id::at::<1>(), key));
    }
    assert!(other.is_empty(Id::at::<0>()));
    assert!(other.is_empty(Id::at::<1>()));
}

#[test]
fn planes_are_independent() {
    let domain = Domain::<claim::Key<'static>, u16>::try_new(257).expect("fixture capacity");
    let (result, counts) = Tracker::measure(|| State::try_new(domain, [Fill::Empty; 5]));
    let mut sets = result.expect("the five 257-key planes must allocate");
    assert_eq!(counts[0], 1);
    let row = sets.try_entry(Ordinal::new(3)).expect("in-domain row");
    row.set(Id::at::<0>());
    row.set(Id::at::<1>());
    assert!(!sets.get::<0>().is_empty());
    assert!(!sets.get::<1>().is_empty());
    let ((), counts) = Tracker::measure(|| {
        {
            let claim = Entry::take_front(&mut sets, Id::at::<0>()).expect("first member");
            assert_eq!(mem::size_of_val(&claim), 2 * mem::size_of::<usize>());
            assert_eq!(claim.index().raw(), 3);
            claim.restore();
        }
        assert!(!sets.get::<0>().is_empty());
        let claim = Entry::take_front(&mut sets, Id::at::<0>()).expect("restored member");
        assert_eq!(claim.index().raw(), 3);
    });
    assert_eq!(counts, [0; 4]);
    assert!(sets.get::<0>().is_empty());
    assert!(!sets.get::<1>().is_empty());
}

#[test]
fn restore_progress() {
    let domain = (Domain::try_new(4)).expect("the fixture capacity must fit");
    let result = State::<(), usize, 1>::try_new(domain, [Fill::Empty]);
    let mut sets = result.expect("the one four-key rollback plane must allocate");
    for index in 0..3 {
        sets.try_entry(Ordinal::new(index))
            .expect("in-domain row")
            .set(Id::at::<0>());
    }
    {
        let claim = Entry::take_front(&mut sets, Id::at::<0>()).expect("first member");
        assert_eq!(claim.index().raw(), 0);
        claim.restore();
    }
    assert!(!sets.get::<0>().is_empty());
    let claim = Entry::take_front(&mut sets, Id::at::<0>()).expect("next member");
    assert_eq!(claim.index().raw(), 1);
    assert!(
        sets.get::<0>()
            .pop_from(Ordinal::new(2))
            .map(|entry| entry.index().raw())
            == Some(2)
    );
    assert!(
        sets.get::<0>()
            .pop_from(Ordinal::new(2))
            .map(|entry| entry.index().raw())
            == Some(0)
    );
}

#[test]
fn transfers_membership() {
    let domain =
        (Domain::<claim::Key<'static>, u16>::try_new(257)).expect("the fixture capacity must fit");
    let (result, counts) = Tracker::measure(|| {
        State::<claim::Key<'static>, u16, 2, Disjoint>::try_disjoint(domain, Id::at::<1>())
    });
    let sets = (result).expect("the two planes must allocate");
    assert_eq!(counts, [1, 112, 0, 0]);
    assert_eq!(mem::size_of_val(&sets), 2 * mem::size_of::<usize>());
    let ((), counts) = Tracker::measure(|| {
        for expected in 0..257 {
            let member =
                (sets.take(Id::at::<1>())).expect("every initialized membership must be available");
            assert_eq!(member.index().raw(), expected);
            member.insert(Id::at::<0>());
        }
        assert!(sets.is_empty(Id::at::<1>()));
        for expected in 0..257 {
            let member =
                (sets.take(Id::at::<0>())).expect("every transferred membership must be retained");
            assert_eq!(member.index().raw(), expected);
            member.insert(Id::at::<1>());
        }
        assert!(sets.is_empty(Id::at::<0>()));
        assert!(!sets.is_empty(Id::at::<1>()));
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn rejects_unbounded() {
    for capacity in [0, 1, 63, 64, 65, 257] {
        let domain = Domain::<(), u16>::try_new(capacity).expect("domain");
        let planes = State::try_new(domain, [Fill::Empty; 3]).expect("planes");
        for index in [capacity, u16::MAX as usize] {
            let error = planes
                .try_entry(Ordinal::new(index as u16))
                .err()
                .expect("outside domain");
            assert_eq!(error, bitmap::Error { index, capacity });
        }
        for plane in [Id::at::<0>(), Id::at::<1>(), Id::at::<2>()] {
            assert!(planes.plane(plane).is_empty());
        }
    }
}

#[test]
fn retains_rows() {
    let domain = Domain::<(), u16>::try_new(65).expect("domain");
    let planes = State::try_new(domain, [Fill::Empty; 3]).expect("planes");
    let entry = planes
        .try_entry(Ordinal::new(64))
        .expect("last bounded row");
    let ((), counts) = Tracker::measure(|| {
        entry.set(Id::at::<0>());
        entry.set(Id::at::<1>());
        entry.clear(Id::at::<0>());
        entry.set(Id::at::<2>());
        entry.clear(Id::at::<1>());
        entry.clear(Id::at::<2>());
        entry.set(Id::at::<2>());
        let taken = planes
            .get::<2>()
            .pop_from(Ordinal::new(64))
            .expect("retained membership");
        assert_eq!(taken.index().raw(), 64);
        let row = taken.into_entry();
        assert_eq!(row.index().raw(), 64);
        row.set(Id::at::<0>());
        row.clear(Id::at::<0>());
        row.set(Id::at::<1>());
    });
    assert_eq!(counts, [0; 4]);
    assert!(planes.get::<0>().is_empty());
    assert!(planes.get::<2>().is_empty());
    assert_eq!(
        planes
            .get::<1>()
            .pop_from(Ordinal::new(0))
            .expect("moved row")
            .index()
            .raw(),
        64
    );
}

#[test]
fn matches_partition() {
    let ids = [Id::<3>::at::<0>(), Id::at::<1>(), Id::at::<2>()];
    for capacity in [0, 1, 63, 64, 65, 4097] {
        let domain = Domain::<(), u16>::try_new(capacity).expect("domain");
        let pool = State::try_disjoint(domain, ids[0]).expect("pool");
        let mut model = vec![0; capacity];
        let ((), counts) = Tracker::measure(|| {
            for step in 0..5000 {
                let source = step % 3;
                let start = (step * 811 + 31) % 65536;
                let found = pool.take_from(ids[source], Ordinal::new(start as u16));
                let expected = (0..capacity)
                    .map(|offset| (start + offset) % capacity)
                    .find(|&index| model[index] == source);
                match (found, expected) {
                    (None, None) => {}
                    (Some(removed), Some(index)) => {
                        assert_eq!(removed.index().raw() as usize, index);
                        let target = (source + 1 + step % 2) % 3;
                        model[index] = target;
                        removed.insert(ids[target]);
                    }
                    (found, expected) => {
                        assert_eq!(found.map(|entry| entry.index().raw() as usize), expected);
                    }
                }
            }
        });
        assert_eq!(counts, [0; 4]);
        for (plane, id) in ids.into_iter().enumerate() {
            for index in (0..capacity).filter(|&index| model[index] == plane) {
                let removed = pool.take_from(id, Ordinal::new(0)).expect("modeled member");
                assert_eq!(removed.index().raw() as usize, index);
            }
            assert!(pool.is_empty(id));
        }
    }
}

#[test]
fn keeps_unique() {
    let domain = Domain::<(), u16>::try_new(2).expect("domain");
    let pool = State::try_disjoint(domain, Id::<2>::at::<0>()).expect("pool");
    let first = pool.take(Id::at::<0>()).expect("first member");
    let second = pool.take(Id::at::<0>()).expect("second member");
    assert_ne!(first.index().raw(), second.index().raw());
    assert!(pool.take(Id::at::<0>()).is_none());
    assert!(pool.take(Id::at::<1>()).is_none());
    second.insert(Id::at::<1>());
    first.insert(Id::at::<1>());
    assert_eq!(
        pool.take(Id::at::<1>())
            .expect("first returned")
            .index()
            .raw(),
        0
    );
    assert_eq!(
        pool.take(Id::at::<1>())
            .expect("second returned")
            .index()
            .raw(),
        1
    );
    assert!(pool.is_empty(Id::at::<1>()));
}

#[test]
fn observes_membership() {
    let domain = Domain::<(), u16>::try_new(65).expect("domain");
    let sets = State::try_disjoint(domain, Id::<2>::at::<0>()).expect("sets");
    let last = Ordinal::new(64);
    let removed = sets.take_from(Id::at::<0>(), last).expect("last member");
    assert!(!sets.contains(Id::at::<0>(), last));
    removed.insert(Id::at::<1>());
    assert!(sets.contains(Id::at::<1>(), last));
    assert!(!sets.contains(Id::at::<1>(), Ordinal::new(65)));
    assert_eq!(
        sets.take(Id::at::<1>())
            .expect("retained member")
            .index()
            .raw(),
        last.raw()
    );
}

#[test]
fn domain_bound() {
    Domain::<(), u8>::try_new(256)
        .as_ref()
        .expect("required fixture outcome");
    assert!(Domain::<(), u8>::try_new(257).is_err());
    const FULL: Result<Domain<(), u16>, capacity::Error> = Domain::try_new(1 << 16);
    assert_eq!(FULL.expect("complete u16 domain").capacity(), 1 << 16);
    assert_eq!(
        Domain::<(), u8>::try_new(0)
            .expect("empty domain")
            .capacity(),
        0
    );
    let error = Domain::<(), u16>::try_new((1 << 16) + 1)
        .err()
        .expect("outside the last index");
    assert_eq!(error.requested(), (1 << 16) + 1);
    if usize::MAX > u32::MAX as usize {
        assert_eq!(
            Domain::<(), usize>::try_new(usize::MAX)
                .err()
                .expect("outside the count domain")
                .requested(),
            usize::MAX
        );
    }
}

#[test]
fn transfers_round_robin() {
    let domain = (Domain::<Key, u16>::try_new(130)).expect("the fixture capacity must fit");
    let pool = State::<Key, u16, 2, set::Disjoint>::try_disjoint(domain, Id::at::<0>())
        .expect("the 130-key pool must allocate");
    let ((), counts) = Tracker::measure(|| {
        for index in [3, 65, 129] {
            let removed = (pool.take_from(Id::at::<0>(), Ordinal::new(index)))
                .expect("the full source must contain the requested member");
            removed.insert(Id::at::<1>());
        }
        for (start, expected) in [(64, 65), (100, 129), (100, 3)] {
            let removed = (pool.take_from(Id::at::<1>(), Ordinal::new(start)))
                .expect("the target must contain the expected member");
            assert_eq!(removed.index().raw(), expected);
        }
    });
    assert_eq!(counts, [0; 4]);
    assert!(pool.is_empty(Id::at::<1>()));
}

#[test]
fn full_dense_words() {
    let domain = (Domain::<(), u16>::try_new(130)).expect("the fixture capacity must fit");
    let set = (State::<(), u16, 1, set::Disjoint>::try_disjoint(domain, Id::at::<0>()))
        .expect("the full 130-key dense set must allocate");
    let last = (set.take_from(Id::at::<0>(), index::Ordinal::new(129)))
        .expect("the full set must contain its last member");
    assert_eq!(last.index().raw(), 129);
    let first = (set.take_from(Id::at::<0>(), index::Ordinal::new(129)))
        .expect("the full set must wrap to its first member");
    assert_eq!(first.index().raw(), 0);
    first.restore();
    last.restore();
    let last = (set.take_from(Id::at::<0>(), index::Ordinal::new(129)))
        .expect("the restored last member must be available");
    assert_eq!(last.index().raw(), 129);
}
