pub mod common;

use std::pin::Pin;

use common::{
    alloc::system::{Activity, Tracker},
    buffer::domain::Capacity,
};
use o3::{
    base::cell::region,
    buffer::{
        Residency as _,
        hold::{self, scope},
        resident::{
            self,
            staging::{self, reserve},
            write,
        },
        storage,
        write::{Sink as _, SpareExt as _, consume::Consume as _},
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn keeps_copy_authority() {
    let mut domains = [resident::Domain::new(), resident::Domain::new()];
    region::Token::scope_pinned(Pin::new(&mut domains), |token, domains| {
        let [first, second] = domains.get_mut();
        let first = first.try_budget(4, token.proof()).expect("first authority");
        let second = second
            .try_budget(4, token.proof())
            .expect("second authority");
        let mut retain = hold::Retention::<resident::Shared<'_>>::default();
        let (empty, activity) = Tracker::activity(|| retain.try_copy(&[]));
        assert!(empty.expect("empty copy needs no authority").is_empty());
        assert_eq!(activity, Activity::default());
        assert!(matches!(
            retain.try_copy(b"body"),
            Err(hold::CopyError::Authority(resident::BindError::Unbound))
        ));
        let ((), activity) = Tracker::activity(|| {
            retain.try_bind(first).expect("first binding");
            retain.try_bind(first).expect("same binding");
            assert_eq!(retain.try_bind(second), Err(resident::BindError::Foreign));
        });
        assert_eq!(activity, Activity::default());
        assert_eq!((first.resident(), second.resident()), (0, 0));
        let copy = retain
            .try_copy(b"body")
            .expect("original authority survives rejection");
        assert_eq!(copy.as_slice(), b"body");
        assert_eq!((first.resident(), second.resident()), (4, 0));
        drop(copy);
        assert_eq!(first.resident(), 0);
    });
}

#[test]
fn preserves_copy_failure() {
    Capacity(4)
        .try_scope(|budget| {
            let mut retain = hold::Retention::<resident::Shared<'_>>::default();
            retain.try_bind(budget).expect("copy authority");
            let copy = retain.try_copy(b"abc").expect("initial copy");
            let failure = retain.try_copy(b"de").expect_err("resident limit");
            let cause = match failure {
                hold::CopyError::Resident(resident::AllocError::Budget(cause)) => Some(cause),
                _ => None,
            }
            .expect("resident budget failure");
            let (required, free) = cause.into_parts();
            assert_eq!((required.get(), free), (2, 1));
            let failed = Tracker::fail_after(0, || retain.try_copy(b"d"));
            assert!(matches!(
                failed,
                Err(hold::CopyError::Resident(resident::AllocError::Exhausted(
                    o3::alloc::Exhausted
                )))
            ));
            assert_eq!(copy.as_slice(), b"abc");
            assert_eq!(budget.resident(), 3);
            drop(copy);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident domain");

    let retain = hold::Retention::<storage::Shared>::default();
    let failed = Tracker::fail_after(0, || retain.try_copy(b"body"));
    assert!(matches!(
        failed,
        Err(hold::CopyError::Storage(storage::AllocError::Exhausted(
            o3::alloc::Exhausted
        )))
    ));
    let mut source = *b"body";
    let copy = retain.try_copy(&source).expect("plain copy");
    source.fill(0);
    assert_eq!(copy.as_slice(), b"body");
}

#[test]
fn fills_and_freezes_owner() {
    Capacity(7)
        .try_scope(|budget| {
            {
                let result = storage::Owned::try_allocate(&budget, 7);
                let mut owned = result.expect("the budget must admit seven filled bytes");
                let mut spare = owned.spare();
                spare.fill(b'x');
                spare.commit();
                let ptr = owned.as_ptr();

                assert_eq!(&owned[..], b"xxxxxxx");
                assert_eq!(budget.resident(), 7);
                let result = owned.try_freeze();
                match result {
                    Ok(exact) => {
                        let shared = exact.into_shared();

                        assert_eq!(shared.as_slice(), b"xxxxxxx");
                        assert_eq!(shared.as_slice().as_ptr(), ptr);
                        assert_eq!(shared.resident(), 7);
                    }
                    outcome => {
                        assert!(outcome.is_ok(), "a completely filled owner must be exact");
                        return;
                    }
                }
            }
            assert_eq!(budget.resident(), 0);
        })
        .expect("asserted empty resident domain");
}

#[test]
fn shrinks_empty() {
    Capacity(16)
        .try_scope(|budget| {
            let result = storage::Owned::try_allocate(&budget, 16);
            let mut empty = result.expect("the budget must admit sixteen bytes");
            assert_eq!(budget.resident(), 16);
            empty
                .try_compact()
                .as_ref()
                .expect("required fixture outcome");
            assert_eq!(empty.capacity(), 0);
            assert_eq!(budget.resident(), 0);
        })
        .expect("asserted empty resident domain");
}

#[test]
fn narrows_owned() {
    let result = storage::Shared::try_copy(b"topology");
    let bytes = result.expect("the topology bytes must allocate");
    let ptr = bytes.as_slice().as_ptr();
    Capacity(0)
        .try_scope(|budget| {
            let (scoped, activity) = Tracker::activity(|| resident::Shared::from(bytes));
            assert_eq!(activity, Activity::default());
            assert_eq!(scoped.as_slice(), b"topology");
            assert_eq!(scoped.as_slice().as_ptr(), ptr);
            assert_eq!(scoped.resident(), b"topology".len());
            let owner = scoped.try_detach().expect("independent owner detaches");
            assert_eq!(owner.as_slice(), b"topology");
            assert_eq!(owner.as_slice().as_ptr(), ptr);
            assert_eq!(budget.resident(), 0);
        })
        .expect("asserted empty resident domain");
}

#[test]
fn narrows_borrowed_bytes() {
    let bytes = b"borrowed-scoped".to_vec();
    let ptr = bytes.as_ptr();
    Capacity(0)
        .try_scope(|budget| {
            let (scoped, activity) = Tracker::activity(|| resident::Shared::from(bytes.as_slice()));
            assert_eq!(activity, Activity::default());
            assert_eq!(scoped.as_slice(), b"borrowed-scoped");
            assert_eq!(scoped.as_slice().as_ptr(), ptr);
            assert_eq!(scoped.resident(), 0);
            assert_eq!(budget.resident(), 0);

            let view =
                (scoped.get(9..scoped.as_slice().len())).expect("borrowed projection must fit");
            let part = (view.try_retain()).expect("borrowed projection must retain its source");
            assert_eq!(part.as_slice(), b"scoped");
            assert_eq!(part.resident(), 0);
            let scoped = scoped.try_detach().expect_err("borrowed owner stays bound");
            assert_eq!(scoped.as_slice().as_ptr(), ptr);
            let owner = storage::Shared::try_copy(scoped.as_slice()).expect("copy borrowed owner");
            assert_eq!(owner.as_slice(), bytes);
            assert_ne!(owner.as_slice().as_ptr(), ptr);
        })
        .expect("asserted empty resident domain");
}

#[test]
fn rejects_invalid_domains() {
    let plan = (staging::Plan::new(1, 1)).expect("valid staging capacity");
    assert!(staging::Layout::new(0, plan, 0, 0) == Err(staging::Error::ZeroBuffers));
    assert!(
        matches!(staging::Layout::new(1, plan, usize::MAX, 0), Err(staging::Error::SlotsOverflow { buffers, shared }) if buffers.get() == 1 && shared == usize::MAX)
    );
    if usize::BITS > u32::BITS {
        assert!(
            matches!(staging::Layout::new(1, plan, u32::MAX as usize, 0), Err(staging::Error::SlotsTooLarge { buffers, shared, .. }) if buffers.get() == 1 && shared == u32::MAX as usize)
        );
    }
}

#[test]
fn matches_resident_formula() {
    let plan = (staging::Plan::new(8, 64)).expect("valid staging capacity");

    let layout = (staging::Layout::new(2, plan, 1, 7)).expect("valid staging capacity");

    assert!(layout.plan() == plan);
    assert_eq!(layout.resident().get(), 24 + 128 + 7);
}

#[test]
fn grows_local_overflow() {
    let plan = (staging::Plan::new(8, 64)).expect("valid staging capacity");

    let layout = (staging::Layout::new(2, plan, 1, 0)).expect("valid staging capacity");

    Capacity(layout.resident().get() + 64)
        .try_scope(|budget| {
            let pool = (staging::Pool::try_new(&budget, layout))
                .expect("overflow staging pool must fit its budget");
            let mut buf = (staging::Buf::try_from(&pool)).expect("staging pool reference");

            assert_eq!(budget.resident(), 24);
            assert_eq!(buf.resident(), 0);
            buf.write_slice(b"12345678").expect("warm append");
            assert_eq!(buf.resident(), 8);
            assert_eq!(budget.resident(), 24);
            buf.try_consume(8)
                .expect("initialized eight-byte prefix must be consumable");

            buf.write_slice(b"abcdefghijkl").expect("local spill");
            assert_eq!(budget.resident(), 40);
            buf.try_reserve(32).expect("local reserve");
            assert_eq!(buf.as_slice(), b"abcdefghijkl");
            assert_eq!(budget.resident(), 56);
        })
        .expect("resident fixture budget");
}

#[test]
fn keeps_both_charges() {
    let plan = (staging::Plan::new(8, 64)).expect("valid staging capacity");

    let layout = (staging::Layout::new(1, plan, 1, 0)).expect("valid staging capacity");

    let mut domains = [resident::Domain::new(), resident::Domain::new()];
    region::Token::scope_pinned(Pin::new(&mut domains), |token, domains| {
        let domains = domains.get_mut();
        let [source, target] = domains;
        let source = (source.try_budget(layout.resident().get() + 64, token.proof()))
            .expect("source resident budget");
        let target = (target.try_budget(12, token.proof())).expect("target resident budget");
        let pool = (staging::Pool::try_account(&source, layout))
            .expect("accounted staging pool must fit the source budget");
        let mut buf = (staging::Buf::try_from(&pool)).expect("staging pool reference");

        buf.try_reserve(12).expect("accounted reserve");
        assert_eq!(buf.resident(), 12);
        buf.write_slice(b"abcdefghijkl").expect("accounted append");
        let ptr = buf.as_slice().as_ptr();
        {
            let kept = (scope::Input::new(&buf, &target)
                .defer_all()
                .bind(hold::Policy::new(1, 1))
                .try_into())
            .expect("staged bytes must fit the target budget");

            assert_eq!(kept.as_slice().as_ptr(), ptr);
            assert_eq!(target.resident(), 12);
            buf.try_consume(12)
                .expect("initialized staged prefix must be consumable");
            buf.write_slice(b"mnopqrstuvwx")
                .expect("shared replacement");
            assert_eq!(kept.as_slice(), b"abcdefghijkl");
        }
        assert_eq!(target.resident(), 0);
    });
}

#[test]
fn retains_spill_causes() {
    let plan = (staging::Plan::new(8, 16)).expect("valid staging capacity");

    let layout = (staging::Layout::new(1, plan, 0, 0)).expect("valid staging capacity");

    Capacity(layout.resident().get())
        .try_scope(|budget| {
            let pool =
                (staging::Pool::try_new(&budget, layout)).expect("pool fixture must succeed");
            let mut buf = (staging::Buf::try_from(&pool)).expect("staging pool reference");
            buf.write_slice(b"abcdefgh").expect("warm append");
            let ptr = buf.as_slice().as_ptr();
            {
                let charge = (budget.try_charge(16)).expect("charge fixture must succeed");
                let Err(reserve::Error::Spill {
                    source: write::Cause::Capacity(source),
                    error: resident::AllocError::Budget(error),
                }) = buf.write_slice(b"i")
                else {
                    panic!("spill must preserve both failures");
                };
                let (required, room) = source.into_parts();
                assert_eq!((required.get(), room), (1, 0));
                let (required, room) = error.into_parts();
                assert_eq!(required.get(), 16);
                assert_eq!(room, 0);
                assert_eq!(buf.as_slice(), b"abcdefgh");
                assert_eq!(buf.as_slice().as_ptr(), ptr);
                assert_eq!(budget.resident(), 24);
                assert_eq!(charge.capacity(), 16);
            }
            buf.write_slice(b"i").expect("retry after budget refund");
            assert_eq!(buf.as_slice(), b"abcdefghi");
        })
        .expect("resident fixture budget");
}
