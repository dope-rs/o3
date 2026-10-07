pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{
    cell::{self, Cell},
    mem, ptr,
    rc::Rc,
};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::arena::{Inline, pool::Reused},
};
use o3::depot::{
    arena::pool::{
        recycle::{self, release},
        shared,
    },
    capacity::{self, Count},
};

#[test]
fn shares_and_recycles() {
    let result = shared::Store::try_new(Count::new(1));
    let pool = result.expect("one shared lease slot must allocate");
    let second = {
        let vacancy = (pool.vacancy()).expect("a new one-slot store has one vacancy");
        assert!(pool.vacancy().is_none());
        let slot = vacancy.reserve();
        assert!(pool.vacancy().is_none());
        let first = slot.commit(String::from("terminal cause"));
        let second = (first.try_clone()).expect("the second lease must fit");
        assert!(first.is_same(&second));
        assert_eq!(first.as_str(), "terminal cause");
        assert!(pool.vacancy().is_none());
        second
    };
    assert!(pool.vacancy().is_none());
    {
        let second = second;
        assert_eq!(second.as_str(), "terminal cause");
    }

    let vacancy = (pool.vacancy()).expect("the final lease drop recycles the only slot");
    assert_eq!(vacancy.reserve().commit(String::new()).as_str(), "");
}

#[test]
fn recycles_reservation() {
    let result = shared::Store::<String>::try_new(Count::new(1));
    let pool = result.expect("one shared lease slot must allocate");
    assert!(
        pool.vacancy()
            .map(o3::depot::arena::pool::Vacant::reserve)
            .is_some(),
        "a new one-slot store has one reservation"
    );
    assert!(
        pool.vacancy()
            .map(o3::depot::arena::pool::Vacant::reserve)
            .is_some(),
        "discarding a reservation recycles the only slot"
    );
    let vacancy = (pool.vacancy()).expect("dropping the reservation recycles the only slot");
    assert_eq!(
        vacancy.reserve().commit(String::from("ready")).as_str(),
        "ready"
    );
}

#[test]
fn commits_in_place() {
    let result = shared::Store::try_new(Count::new(1));
    let pool = result.expect("one inline shared lease slot must allocate");
    let ptr = Cell::new(ptr::null());
    let vacancy = (pool.vacancy()).expect("a new one-slot store has one vacancy");
    let mut unique = vacancy.reserve().begin(Inline::default());
    ptr.set(&*unique as *const Inline);
    unique.bytes[..5].copy_from_slice(b"exact");
    unique.len = 5;
    let lease = unique.freeze();
    assert_eq!(ptr.get(), &*lease as *const Inline);
    assert_eq!(&lease.bytes[..lease.len], b"exact");
}

#[test]
fn freezes_in_place() {
    let result = shared::Store::try_new(Count::new(1));
    let pool = result.expect("one string shared lease slot must allocate");
    let vacancy = (pool.vacancy()).expect("a new one-slot store has one vacancy");
    let mut unique = vacancy.reserve().begin(String::from("exact"));
    let ptr = &*unique as *const String;
    unique.push_str(" batch");
    assert!(pool.vacancy().is_none());

    let clone = {
        let lease = unique.freeze();
        assert_eq!(ptr, &*lease as *const String);
        assert_eq!(lease.as_str(), "exact batch");
        let clone = (lease.try_clone()).expect("the second lease must fit");
        assert!(lease.is_same(&clone));
        clone
    };
    assert!(pool.vacancy().is_none());
    {
        let clone = clone;
        assert_eq!(clone.as_str(), "exact batch");
    }
    assert!(pool.vacancy().is_some());
}

#[test]
fn recovers_unique_owner() {
    let result = shared::Store::try_new(Count::new(1));
    let pool = result.expect("one string shared lease slot must allocate");
    let vacancy = (pool.vacancy()).expect("a new one-slot store has one vacancy");
    let lease = vacancy.reserve().commit(String::from("old"));
    let ptr = &*lease as *const String;
    let second = lease.try_clone().expect("the second lease must fit");
    let lease = lease
        .try_unique()
        .err()
        .expect("an aliased shared lease must remain shared");
    assert!(lease.is_same(&second));
    assert_eq!(lease.as_str(), "old");
    drop(second);
    assert!(pool.vacancy().is_none());

    let mut unique = lease
        .try_unique()
        .expect("a sole shared lease must become unique");
    unique.clear();
    unique.push_str("new");
    assert_eq!(ptr, &*unique as *const String);
    assert!(pool.vacancy().is_none());
    {
        let lease = unique.freeze();
        assert_eq!(lease.as_str(), "new");
    }
    assert!(pool.vacancy().is_some());
}

#[test]
fn recycles_dropped_unique() {
    let drops = Rc::new(Cell::new(Some(0)));
    let result = shared::Store::try_new(Count::new(1));
    let pool = result.expect("one counted shared lease slot must allocate");
    {
        let vacancy = (pool.vacancy()).expect("a new one-slot store has one vacancy");
        let unique = vacancy.reserve().begin(Counted(Rc::clone(&drops)));
        assert!(Rc::ptr_eq(&unique.0, &drops));
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(1));
    assert!(pool.vacancy().is_some());
}

#[test]
fn keeps_word_handles() {
    assert_eq!(
        mem::size_of::<shared::Store<usize>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Vacant<'static, shared::Resident<usize>, shared::Header<usize>>,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<
                o3::depot::arena::pool::Vacant<
                    'static,
                    shared::Resident<usize>,
                    shared::Header<usize>,
                >,
            >,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Unique<'static, shared::Resident<usize>, shared::Header<usize>>,
        >(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Unique<
                'static,
                shared::Resident<usize>,
                shared::Header<usize>,
                shared::Initialized,
            >,
        >(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<
                o3::depot::arena::pool::Unique<
                    'static,
                    shared::Resident<usize>,
                    shared::Header<usize>,
                    shared::Initialized,
                >,
            >,
        >(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Shared<
                'static,
                shared::Resident<usize>,
                shared::Header<usize>,
                shared::Initialized,
            >,
        >(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<
                o3::depot::arena::pool::Shared<
                    'static,
                    shared::Resident<usize>,
                    shared::Header<usize>,
                    shared::Initialized,
                >,
            >,
        >(),
        mem::size_of::<usize>()
    );
}

#[test]
fn publishes_pair() {
    let drops = Cell::new(Some(0));
    let store = (shared::Store::try_new(Count::new(1))).expect("one shared slot must allocate");
    let first = {
        let vacancy = (store.vacancy()).expect("fresh slot must be vacant");
        let unique = vacancy.reserve().begin(Counted(&drops));
        let (first, second) = unique.freeze_pair();
        assert!(first.is_same(&second));
        assert!(store.vacancy().is_none());
        let result = first.try_unique();
        assert!(result.is_err(), "two published owners cannot be unique");
        match result {
            Err(error) => error,
            Ok(_) => return,
        }
    };
    assert_eq!(drops.get(), Some(0));
    match first.try_unique() {
        Ok(unique) => {
            {
                let pair = unique.freeze_pair();
                {
                    let first = pair.0;
                    assert!(first.is_same(&pair.1));
                }
                assert_eq!(drops.get(), Some(0));
            }
            assert_eq!(drops.get(), Some(1));
            assert!(store.vacancy().is_some());
        }
        outcome => assert!(outcome.is_ok(), "remaining shared edge must be unique"),
    }
}

#[test]
fn shared_is_cold() {
    let (created, build) = Tracker::measure(|| shared::Store::try_new(capacity::Count::new(2)));
    assert_eq!(build[0], 1);
    assert_eq!(build[2], 0);
    let pool = (created).expect("the measured two-slot shared pool must allocate");

    let ((), hot) = Tracker::measure(|| {
        {
            let vacancy = (pool.vacancy()).expect("a fresh two-slot shared pool has a vacancy");
            let first = vacancy.reserve().commit(7_u64);
            let second = (first.try_clone()).expect("the occupied slot must admit its second edge");
            assert_eq!(*first, 7);
            assert_eq!(*second, 7);
        }
        let vacancy = (pool.vacancy()).expect("released shared slots return to the pool");
        let (first, second) = vacancy.reserve().begin(0_u64).freeze_pair();
        assert!(first.is_same(&second));
        assert_eq!(*first, 0);
        assert_eq!(*second, 0);
    });
    assert_eq!(hot, [0; 4]);

    let bytes = build[1];
    let ((), freed) = Tracker::measure(|| {
        let released = pool;
        assert!(released.vacancy().is_some());
    });
    assert_eq!(freed, [0, 0, 1, bytes]);
}

#[test]
fn recycle_is_cold() {
    let (created, build) =
        Tracker::measure(|| recycle::Store::<u64>::try_new(capacity::Count::new(2)));
    assert_eq!(build[0], 1);
    assert_eq!(build[2], 0);
    let pool = (created).expect("the measured two-slot recycle pool must allocate");

    let ((), hot) = Tracker::measure(|| {
        {
            let mut lease = (pool.take()).expect("a fresh recyclable pool has a slot");
            *lease.get_mut().get_mut() = 11;
            assert_eq!(*lease.get(), 11);
        }
        let lease = (pool.take()).expect("the released recyclable slot returns to the pool");
        assert_eq!(*lease.get(), 11);
    });
    assert_eq!(hot, [0; 4]);

    let bytes = build[1];
    let ((), freed) = Tracker::measure(|| {
        let released = pool;
        assert!(released.take().is_some());
    });
    assert_eq!(freed, [0, 0, 1, bytes]);
}

#[test]
fn drops_rejected_context() {
    let drops = cell::Cell::new(Some(0));
    let created = Tracker::fail_after(0, || {
        release::Store::<Reused, _>::try_init(capacity::Count::new(1), Counted(&drops), ())
    });
    assert!(matches!(created, Err(o3::alloc::Error::Exhausted)));
    assert_eq!(drops.get(), Some(1));
}
