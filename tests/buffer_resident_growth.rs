pub mod common;

use std::pin::Pin;

use common::{alloc::system::Tracker, buffer::domain::Capacity};
use o3::{
    alloc,
    base::cell::region,
    buffer::{resident, storage},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn grows_and_refunds() {
    Capacity(8)
        .try_scope(|budget| {
            let mut owner = storage::Owned::try_allocate(&budget, 0).expect("empty owner");
            let ((), count) = Tracker::count(|| owner.try_reserve(&budget, 0).expect("no growth"));
            assert_eq!(count, 0);
            owner.try_reserve(&budget, 4).expect("initial storage");
            owner.try_extend(b"abc").expect("initialized prefix");
            let ((), count) = Tracker::resizes(|| owner.try_reserve(&budget, 8).expect("delta"));
            assert_eq!(count, 1);
            assert_eq!((owner.capacity(), budget.resident()), (8, 8));
            assert_eq!(owner.as_slice(), b"abc");
            owner.try_extend(b"de").expect("new spare bytes");
            let frozen = owner.freeze();
            assert_eq!(frozen.as_slice(), b"abcde");
            drop(frozen);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident domain");
}

#[test]
fn rejects_without_change() {
    Capacity(8)
        .try_scope(|budget| {
            let mut owner = storage::Owned::try_allocate(&budget, 4).expect("initial storage");
            owner.try_extend(b"abc").expect("initialized prefix");
            let pointer = owner.as_ptr();
            assert!(matches!(
                owner.try_reserve(&budget, 9),
                Err(resident::AllocError::Budget(_))
            ));
            assert_eq!(
                Tracker::fail_after(0, || owner.try_reserve(&budget, 8)),
                Err(resident::AllocError::Exhausted(alloc::Exhausted))
            );
            assert!(matches!(
                owner.try_reserve(&budget, usize::MAX),
                Err(resident::AllocError::Capacity(_))
            ));
            assert_eq!((owner.as_ptr(), owner.capacity()), (pointer, 4));
            assert_eq!((owner.as_slice(), budget.resident()), (&b"abc"[..], 4));
            owner.try_reserve(&budget, 8).expect("retry after refund");
            drop(owner);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident domain");
}

#[test]
fn retains_growth_domain() {
    let mut domains = (resident::Domain::new(), resident::Domain::new());
    region::Token::scope_pinned(Pin::new(&mut domains), |token, domains| {
        let (first, second) = domains.get_mut();
        let seed = first.try_budget(8, token.proof()).expect("first domain");
        let other = second.try_budget(1, token.proof()).expect("second domain");
        let mut owner = storage::Owned::try_allocate(&seed, 0).expect("empty owner");
        owner.try_reserve(&seed, 4).expect("bind first domain");
        owner.try_reserve(&other, 8).expect("grow original domain");
        assert_eq!((seed.resident(), other.resident()), (8, 0));
        drop(owner);
        assert_eq!(seed.resident(), 0);
    });
}
