pub mod common;

use common::{
    alloc::system::Tracker,
    depot::{index::key::Id, map::Sent},
};
use o3::depot::{self, capacity, index::Initialized, map::direct, queue::links, raw::Key as _};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn retains_key_and_plane() {
    let mut table = (direct::Table::<Id<'_>, u64>::try_new(capacity::Count::new(1)))
        .expect("direct table must allocate");
    let key = Id::try_new(0, 7).expect("identity components fit");
    let vacant = (table.vacant(key)).expect("initial direct slot must be vacant");
    vacant.insert(31);
    let mut count: usize = 5;
    let (observed, delta) = Tracker::measure(|| {
        // SAFETY: the checked insertion initialized this exact key in the retained table.
        let live: depot::raw::Proof<_, Initialized<Sent>> =
            unsafe { depot::raw::Proof::new((&mut table, key)) };
        // SAFETY: this shared projection keeps the original table, initialized slot, and plane.
        let read = unsafe {
            live.as_ref().map(|(table, key)| {
                (
                    &**table,
                    <links::Index as depot::raw::Word>::from_offset(key.index()),
                )
            })
        };
        let read: depot::raw::Proof<(&u64, Id<'_>), Initialized<Sent>> =
            direct::Table::project(read);
        assert!(*read.key() == key);
        assert_eq!(**read.source(), 31);
        // SAFETY: this consumes the same original owner and keeps its caller context and plane.
        let live = unsafe {
            live.map(|(table, key)| {
                (
                    (table, &mut count),
                    <links::Index as depot::raw::Word>::from_offset(key.index()),
                )
            })
        };
        let (slot, count): (direct::Occupied<'_, Id<'_>, u64, Sent>, _) =
            direct::Occupied::from_live(live);
        let observed = slot.key();
        let (vacant, value): (direct::Vacant<'_, Id<'_>, u64, Sent>, _) = slot.take();
        let value = value + 11;
        *count = count.checked_add(1).expect("caller context increment");
        vacant.insert(value);
        observed
    });
    assert_eq!(delta, [0; 4]);
    assert!((observed, count) == (key, 6));
    assert_eq!(table.len(), 1);
    assert_eq!(table.get(key), Some(&42));
    assert!(
        table
            .entry(Id::try_new(0, 8).expect("identity components fit"))
            .is_none()
    );
    assert!(
        table
            .entry(Id::try_new(1, 7).expect("identity components fit"))
            .is_none()
    );
    let slot = (table.entry(key)).expect("restored identity must remain occupied");
    assert_eq!(slot.remove(), 42);
    assert!(table.is_empty());
    assert!(table.get(key).is_none());
    assert_eq!(size_of::<direct::Occupied<'_, Id<'_>, u64, Sent>>(), 16);
    assert_eq!(
        size_of::<Option<direct::Occupied<'_, Id<'_>, u64, Sent>>>(),
        16
    );
    assert_eq!(size_of::<direct::Vacant<'_, Id<'_>, u64, Sent>>(), 16);
    assert_eq!(
        size_of::<Option<direct::Vacant<'_, Id<'_>, u64, Sent>>>(),
        16
    );
}

#[test]
fn reports_observed_slot() {
    let mut table = direct::Table::try_new(capacity::Count::new(2)).expect("table allocates");
    let key = Id::try_new(0, 1).expect("identity components fit");
    {
        let occupied = table
            .try_vacant(key)
            .expect("direct key must retain its complete identity")
            .occupy(7);
        assert!(occupied.key() == key);
        assert_eq!(*occupied.get(), 7);
    }

    let ((), counts) = Tracker::measure(|| {
        for wanted in [key, Id::try_new(0, 2).expect("identity components fit")] {
            let error = table.try_vacant(wanted).err().expect("occupied slot");
            assert!(matches!(
                error,
                direct::Error::Occupied { key: actual, stored }
                    if actual == wanted && stored == key
            ));
        }
        let wanted = Id::try_new(2, 3).expect("identity components fit");
        let error = table.try_vacant(wanted).err().expect("outside slot");
        assert!(matches!(
            error,
            direct::Error::Outside { key, index: 2, capacity: 2 } if key == wanted
        ));
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(table.len(), 1);
    assert_eq!(table.get(key), Some(&7));
    assert_eq!(size_of::<direct::Error<Id<'_>>>(), 3 * size_of::<usize>());
}

#[test]
fn retains_empty_domain() {
    let mut table = direct::Table::<Id<'_>, u64>::try_new(capacity::Count::EMPTY)
        .expect("empty domain allocates no storage");
    let wanted = Id::try_new(0, 1).expect("identity components fit");
    let (error, counts) = Tracker::measure(|| {
        table
            .try_vacant(wanted)
            .err()
            .expect("empty domain has no vacant slot")
    });
    assert!(matches!(
        error,
        direct::Error::Outside { key, index: 0, capacity: 0 } if key == wanted
    ));
    assert_eq!(counts, [0; 4]);
    assert!(table.is_empty());
}
