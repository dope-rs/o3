pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, num::NonZero};

use common::{
    alloc::system::{Activity, Tracker},
    buffer::{Counted, domain::Capacity, try_item},
};
use o3::buffer::{
    self, Residency as _,
    hold::{self, Charge, Overflow, ranges},
    segments, storage,
    write::consume::scope,
};

#[test]
fn validates_at_source() {
    use hold::scope::Input;
    let source = Counted {
        bytes: *b"abc",
        reads: Cell::new(Some(0)),
    };
    Capacity(0)
        .try_scope(|budget| {
            let input = Input::new(&source, &budget);
            let result = input.try_plan(0..4);
            assert!(result.is_err(), "the observed range exceeds the source");
            let Err(error) = result else { return };
            assert_eq!((error.start(), error.end(), error.len()), (0, 4, 3));
            assert_eq!(source.reads.get(), Some(1));
            let plan = (input.try_plan(0..3)).expect("the complete source must be admitted");
            assert_eq!(source.reads.get(), Some(2));
            assert_eq!(plan.as_slice(), b"abc");
            assert_eq!(plan.as_slice().as_ptr(), source.bytes.as_ptr());
            assert_eq!(source.reads.get(), Some(2));
            let (split, activity) = Tracker::activity(|| plan.try_split::<2>());
            assert!(activity == Activity::default());
            let (head, tail) = (split).expect("the two-byte header fits");
            assert_eq!(head, b"ab");
            assert_eq!(head.as_ptr(), source.bytes.as_ptr());
            assert_eq!(tail.as_slice(), b"c");
            let result = tail.try_split::<2>();
            assert!(
                result.is_err(),
                "the one-byte remainder cannot contain two bytes"
            );
            let Err(cause) = result else { return };
            assert_eq!(
                cause.into_parts(),
                (NonZero::<usize>::MIN.saturating_add(1), 1)
            );
            assert_eq!(
                head, b"ab",
                "the original borrow outlives the consumed plan"
            );
            assert_eq!(source.reads.get(), Some(2));
            input.scope(|input| {
                let count = NonZero::<usize>::MIN.saturating_add(2);
                assert!(matches!(
                    input.try_prefix::<2>(count),
                    Err(scope::Error::Maximum { count: observed, maximum: 2 }) if observed == count
                ));
            });
            assert_eq!(source.reads.get(), Some(2));
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn prefix_borrows_source() {
    let source = (storage::Shared::try_copy(b"abcdef")).expect("source allocation");
    let (prefix, activity) = Tracker::activity(|| {
        scope::Input::new(&source).scope(|input| {
            let count = NonZero::<usize>::MIN.saturating_add(2);
            input
                .try_prefix::<{ u32::MAX }>(count)
                .map(|(prefix, end)| {
                    assert_eq!(end.count().get() as usize, count.get());
                    prefix
                })
        })
    });
    assert!(activity == Activity::default());
    let prefix = (prefix).expect("the prefix fits its source");
    assert_eq!(prefix.as_slice(), b"abc");
    assert_eq!(prefix.as_slice().as_ptr(), source.as_ptr());
    assert_eq!(source.as_slice(), b"abcdef");
}

#[test]
fn consume_own_prefix() {
    let mut source = (storage::Shared::try_copy(b"abcdef")).expect("source allocation");
    let start = source.as_ptr();
    let (result, activity) = Tracker::activity(|| {
        scope::Input::consume(&mut source, |input| {
            match input.try_prefix::<6>(NonZero::<usize>::MIN.saturating_add(2)) {
                Ok((prefix, end)) => {
                    assert_eq!(prefix.as_slice(), b"abc");
                    (Some(end), Ok(()))
                }
                Err(error) => (None, Err(error)),
            }
        })
    });
    result.as_ref().expect("required fixture outcome");
    assert!(activity == Activity::default());
    assert_eq!(source.as_slice(), b"def");
    assert_eq!(source.as_ptr(), start.wrapping_add(3));

    scope::Input::consume::<_, 6>(&mut source, |input| {
        let admitted = input.try_prefix::<6>(NonZero::<usize>::MIN);
        admitted.as_ref().expect("required fixture outcome");
        (None, ())
    });
    assert_eq!(source.as_slice(), b"def");

    let rejected = scope::Input::consume::<_, 6>(&mut source, |input| {
        match input.try_prefix::<6>(NonZero::<usize>::MIN.saturating_add(3)) {
            Err(error) => (None, Some(error)),
            Ok(_) => (None, None),
        }
    });
    assert!(matches!(rejected, Some(scope::Error::Range(error))
            if (error.start(), error.end(), error.len()) == (0, 4, 3)));
    assert_eq!(source.as_slice(), b"def");
}

#[test]
fn commit_or_cancel_pooled() {
    use buffer::{Cursor as _, bytes, pool};

    let pool = (pool::Pool::try_new(1, 16)).expect("pooled source allocation");
    let mut writer = (pool.view()).expect("one free pooled slot");
    {
        let mut spare = writer.spare();
        assert_eq!(spare.write(b"abcdef"), 6);
        spare.commit();
    }
    let mut source = writer.freeze();
    let ((), activity) = Tracker::activity(|| {
        for commit in [false, true] {
            let mut cursor = bytes::Cursor::new(&mut source);
            scope::Input::consume(&mut cursor, |input| {
                let (prefix, end) = (input
                    .try_prefix::<6>(NonZero::<usize>::MIN.saturating_add(1)))
                .expect("the first two bytes fit");
                assert_eq!(prefix.as_slice(), b"ab");
                (Some(end), ())
            });
            let plan = cursor.try_get(0..4).expect("the remaining four bytes fit");
            assert_eq!(plan.as_slice(), b"cdef");
            if commit {
                cursor.commit();
            }
        }
    });
    assert!(activity == Activity::default());
    assert_eq!(source.as_slice(), b"cdef");
}

#[test]
fn consume_keeps_source() {
    use buffer::write::consume::Consume as _;

    let source = storage::Shared::try_copy(b"abcdef").expect("shared source allocation");
    let mut view = buffer::View::new(&source)
        .try_get(1..5)
        .expect("the middle four bytes fit");
    let ((), activity) = Tracker::activity(|| {
        view.try_consume(1)
            .expect("one byte fits the selected view");
        let result = view.try_consume(usize::MAX);
        assert!(
            result.is_err(),
            "an oversized consume preserves the selected suffix"
        );
        let Err(error) = result else { return };
        let (required, left) = error.into_parts();
        assert_eq!((required.get(), left), (usize::MAX, 3));
        assert_eq!(view.as_slice(), b"cde");
        assert_eq!(view.as_slice().as_ptr(), source.as_slice()[2..].as_ptr());
        view.try_consume(0).expect("an empty prefix always fits");
        assert_eq!(view.consume_max(usize::MAX), 3);
        view.try_consume(0)
            .expect("an empty view admits an empty prefix");
        assert!(view.is_empty());
    });
    assert!(activity == Activity::default());
    assert_eq!(source.as_slice(), b"abcdef");
}

#[test]
fn reports_overflow() {
    let logical = Charge::from_commit(usize::MAX, 3, 5);
    assert!(
        logical.try_add(Charge::from_commit(1, 7, 11))
            == Err(Overflow::Logical {
                left: usize::MAX,
                right: 1,
            })
    );

    let resident = Charge::from_commit(2, usize::MAX, 5);
    assert!(
        resident.try_add(Charge::from_commit(3, 1, 11))
            == Err(Overflow::Resident {
                left: usize::MAX,
                right: 1,
            })
    );

    let owners = Charge::from_commit(2, 3, usize::MAX);
    assert!(
        owners.try_add(Charge::from_commit(5, 7, 1))
            == Err(Overflow::Owners {
                left: usize::MAX,
                right: 1,
            })
    );
}

#[test]
fn projects_retained() {
    let owned = try_item(b"owned").expect("the retained chain fixture must allocate");
    let resident = owned.resident();
    let retained = segments::Item::from(owned.try_clone().expect("retained alias"));
    let borrowed = segments::Item::from(b"borrowed");
    let owned = segments::Item::from(owned);
    let chain = (segments::Many::try_from((owned, borrowed))).expect("segment transition accepted");
    let chain = segments::Many::try_from((chain, retained)).expect("retained owner appended");

    assert!(Charge::from(chain.as_chain()) == Charge::from_commit(10, resident * 2, 2));
}

#[test]
fn retains_after_plans() {
    let (direct, iterated) = {
        let source = storage::Shared::try_copy(b"_ab__cde_").expect("source allocation");
        let ((direct, iterated, slice), activity) = Tracker::measure(|| {
            let spans = [(1u32, 3), (5, 8)];
            // SAFETY: these fixed nonempty ranges select the unchanged source
            // at its original origin, and their exact total is five bytes.
            let plan = unsafe { ranges::Set::from_parts(&source, &spans, 5) };
            let direct = plan.get(1).expect("second range");
            let iterated = plan.iter().next().expect("first range");
            let span = [(1, 3)];
            let single = ranges::Set::<_>::try_new(&source, &span, 2).expect("contiguous range");
            (direct, iterated, single.contiguous().expect("one range"))
        });
        assert_eq!(activity, [0; 4]);
        assert_eq!(direct.as_slice(), b"cde");
        assert_eq!(iterated.as_slice(), b"ab");
        assert_eq!(slice, b"ab");
        assert_eq!(direct.as_slice().as_ptr(), source.as_slice()[5..].as_ptr());
        assert_eq!(
            iterated.as_slice().as_ptr(),
            source.as_slice()[1..].as_ptr()
        );
        assert_eq!(slice.as_ptr(), source.as_slice()[1..].as_ptr());

        let ((direct, iterated), activity) = Tracker::measure(|| {
            (
                direct.try_retain().expect("selected owner"),
                iterated.try_retain().expect("iterated owner"),
            )
        });
        assert_eq!(activity, [0; 4]);
        (direct, iterated)
    };
    assert_eq!(direct.as_slice(), b"cde");
    assert_eq!(iterated.as_slice(), b"ab");
}
