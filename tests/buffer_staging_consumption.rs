pub mod common;

use std::{mem, num::NonZero, ops};

use common::{
    alloc::system::Tracker,
    buffer::{Local, domain::Capacity},
};
use o3::buffer::{
    Residency as _,
    hold::scope::Input,
    resident::{
        self,
        staging::{self, reserve},
    },
    write::{
        Sink as _,
        consume::{Consume as _, scope},
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn preserves_owner_policy() {
    let plan = staging::Plan::new(1, 8).expect("valid staging capacity");
    let layout = staging::Layout::new(1, plan, 0, 12).expect("valid staging layout");
    for shared in [false, true] {
        for capacity in [1, 4] {
            for path in 0..3 {
                Capacity(layout.resident().get())
                    .try_scope(|budget| {
                        let pool = if shared {
                            staging::Pool::try_account(&budget, layout)
                        } else {
                            staging::Pool::try_new(&budget, layout)
                        }
                        .expect("staging pool allocation");
                        let mut source = staging::Buf::try_from(&pool).expect("staging owner");
                        source.try_reserve(capacity).expect("exact reservation");
                        source
                            .write_slice(&b"abcd"[..capacity])
                            .expect("source fits");
                        let before = budget.resident();
                        let pointer = source.as_slice().as_ptr();
                        let (rejected, activity) =
                            Tracker::measure(|| source.try_consume(capacity + 1));
                        assert_eq!(activity, [0; 4]);
                        assert!(matches!(rejected, Err(error)
                        if error.into_parts() == (
                            NonZero::new(capacity + 1).expect("positive excess"), capacity,
                        )));
                        assert_eq!(source.as_slice(), &b"abcd"[..capacity]);
                        assert_eq!(source.as_slice().as_ptr(), pointer);
                        assert_eq!(budget.resident(), before);
                        match path {
                            0 => source.try_consume(capacity).expect("complete prefix"),
                            1 => assert_eq!(source.consume_max(usize::MAX), capacity),
                            _ => scope::Input::consume::<_, 8>(&mut source, |input| {
                                let (_, end) = input
                                    .try_prefix(NonZero::new(capacity).expect("nonempty source"))
                                    .expect("complete prefix");
                                (Some(end), ())
                            }),
                        }
                        let released = !shared && capacity > 1;
                        assert!(source.is_empty());
                        assert_eq!(source.resident(), if released { 0 } else { capacity });
                        assert_eq!(budget.resident(), before - usize::from(released) * capacity);
                        source.try_reserve(capacity).expect("empty reservation");
                        let before = budget.resident();
                        let ((), activity) = Tracker::measure(|| {
                            source.try_consume(0).expect("empty prefix");
                            assert_eq!(source.consume_max(0), 0);
                            assert_eq!(source.consume_max(usize::MAX), 0);
                            scope::Input::consume::<_, 8>(&mut source, |_| (None, ()));
                        });
                        assert_eq!(activity, [0; 4]);
                        assert_eq!(source.resident(), capacity);
                        assert_eq!(budget.resident(), before);
                        source
                            .write_slice(&b"efgh"[..capacity])
                            .expect("restart fits");
                        assert_eq!(source.as_slice(), &b"efgh"[..capacity]);
                    })
                    .expect("resident domain admission");
            }
        }
    }
}

#[test]
fn retains_ranges() {
    use std::num::NonZero;

    use o3::buffer::{self, Complete as _, Cursor as _, Read as _, hold};

    for account in [false, true] {
        for capacity in [4, 8] {
            Capacity(64)
                .try_scope(|budget| {
                    let (retained, pointer) = {
                        let plan = (staging::Plan::new(4, 8)).expect("valid staging capacity");

                        let layout =
                            (staging::Layout::new(1, plan, 1, 8)).expect("valid staging capacity");

                        let created = if account {
                            staging::Pool::try_account(&budget, layout)
                        } else {
                            staging::Pool::try_new(&budget, layout)
                        };
                        let pool = (created).expect("staging pool");
                        let mut source = (staging::Buf::try_from(&pool)).expect("staging buffer");
                        empty(&mut source, &budget);
                        source
                            .try_reserve(capacity)
                            .expect("staging reservation fits");
                        empty(&mut source, &budget);
                        source.write_slice(b"abcd").expect("staging append fits");
                        let pointer = source.as_slice().as_ptr().wrapping_add(2);
                        let (retained, activity) = Tracker::measure(|| {
                            let view = (source.try_get(1..4)).expect("checked source range");
                            let (head, tail) =
                                (view.try_split::<1>()).expect("prefix fits source range");
                            assert_eq!(head, b"b");
                            assert_eq!(tail.as_slice(), b"cd");
                            assert_eq!(tail.as_slice().as_ptr(), pointer);
                            let charge = <staging::Buf<'_> as hold::raw::Retain<'_>>::charge(&tail);
                            assert!(charge == hold::Charge::from_commit(2, capacity, 1));
                            <staging::Buf<'_> as hold::raw::Retain<'_>>::try_retain(&tail, &budget)
                        });
                        assert_eq!(activity, [0; 4]);
                        let retained = retained.expect("retain checked backing without copying");
                        let (head, tail) =
                            (source.view().try_split::<4>()).expect("split through the source end");
                        assert_eq!(head, b"abcd");
                        assert_eq!(tail.as_slice(), b"");
                        assert!(
                            <staging::Buf<'_> as hold::raw::Retain<'_>>::charge(&tail)
                                == hold::Charge::default()
                        );
                        let (result, activity) = Tracker::measure(|| {
                            <staging::Buf<'_> as hold::raw::Retain<'_>>::try_retain(&tail, &budget)
                        });
                        assert_eq!(activity, [0; 4]);
                        assert!(matches!(result, Ok(ref bytes) if bytes.is_empty()));
                        let ((), activity) = Tracker::measure(|| {
                            Input::consume::<_, 1>(&mut source, &budget, |input| {
                                let (prefix, end) = (input.try_prefix(NonZero::<usize>::MIN))
                                    .expect("source-bound partial consumption");
                                assert_eq!(prefix.as_slice(), b"a");
                                (Some(end), ())
                            });
                            assert_eq!(source.as_slice(), b"bcd");
                            Input::consume::<_, 4>(&mut source, &budget, |input| {
                                let (prefix, end) = (input
                                    .try_prefix(NonZero::<usize>::MIN.saturating_add(2)))
                                .expect("source-bound consumption");
                                assert_eq!(prefix.as_slice(), b"bcd");
                                (Some(end), ())
                            });
                        });
                        assert_eq!(activity, [0; 4]);
                        empty(&mut source, &budget);
                        source.clear();
                        empty(&mut source, &budget);
                        (retained, pointer)
                    };
                    assert_eq!(retained.as_slice(), b"cd");
                    assert_eq!(retained.as_slice().as_ptr(), pointer);
                    assert_eq!(retained.resident(), capacity);
                    assert_eq!(
                        mem::size_of::<buffer::View<'static, staging::Buf<'static>>>(),
                        3 * mem::size_of::<usize>()
                    );
                    assert_eq!(
                        mem::size_of::<Option<buffer::View<'static, staging::Buf<'static>>>>(),
                        3 * mem::size_of::<usize>()
                    );
                    assert_eq!(
                        mem::size_of::<
                            hold::scope::Deferred<'static, 'static, staging::Buf<'static>>,
                        >(),
                        4 * mem::size_of::<usize>()
                    );
                    assert_eq!(
                        mem::size_of::<hold::scope::Bound<'static, 'static, staging::Buf<'static>>>(
                        ),
                        5 * mem::size_of::<usize>()
                    );
                    assert_eq!(
                        mem::size_of::<
                            Result<
                                <staging::Buf<'static> as buffer::Source>::View<'static>,
                                buffer::storage::range::Error,
                            >,
                        >(),
                        4 * mem::size_of::<usize>()
                    );
                })
                .expect("resident test domain must admit its budget");
        }
    }
}

fn empty<'d>(source: &mut staging::Buf<'d>, budget: &resident::Budget<'d>) {
    use std::num::NonZero;

    use o3::buffer::{Complete as _, Cursor as _, Read as _, hold, write::consume::scope};

    let before = budget.resident();
    let ((), activity) = Tracker::measure(|| {
        let view = source.view();
        assert_eq!(view.as_slice(), b"");
        let (head, tail) = (view.try_split::<0>()).expect("zero prefix of empty source");
        assert_eq!(head, b"");
        assert_eq!(tail.as_slice(), b"");
        assert_eq!(
            source.try_get(0..0).expect("empty range fits").as_slice(),
            b""
        );
        for range in [
            0..1,
            1..1,
            ops::Range { start: 2, end: 1 },
            usize::MAX..usize::MAX,
        ] {
            let result = source.try_get(range.clone());
            assert!(result.is_err(), "reject range outside empty source");
            let Err(error) = result else { return };
            assert_eq!(
                (error.start(), error.end(), error.len()),
                (range.start, range.end, 0)
            );
        }
        let result = source.view().try_split::<1>();
        assert!(result.is_err(), "empty source cannot provide one byte");
        let Err(error) = result else { return };
        let (required, capacity) = error.into_parts();
        assert_eq!((required.get(), capacity), (1, 0));
        assert!(
            <staging::Buf<'_> as hold::raw::Retain<'_>>::charge(&tail) == hold::Charge::default()
        );
        let result = <staging::Buf<'_> as hold::raw::Retain<'_>>::try_retain(&tail, budget);
        assert!(matches!(result, Ok(ref bytes) if bytes.is_empty() && bytes.resident() == 0));
        scope::Input::consume::<_, 1>(source, |input| {
            let result = input.try_prefix::<1>(NonZero::<usize>::MIN);
            assert!(matches!(result, Err(scope::Error::Range(error))
                if (error.start(), error.end(), error.len()) == (0, 1, 0)));
            (None, ())
        });
    });
    assert_eq!(activity, [0; 4]);
    assert_eq!(budget.resident(), before);
}

#[test]
fn restarts_local_storage() {
    let fixture = Local::try_new().expect("local staging geometry");
    Capacity(fixture.0.resident().get())
        .try_scope(|budget| {
            let mut source = fixture.try_buf(&budget).expect("local staging fixture");
            assert_eq!(source.consume_max(4), 4);
            assert_eq!(source.resident(), 0);
            empty(&mut source, &budget);
            for result in [
                Tracker::fail_after(0, || source.write_slice(b"e")),
                Tracker::fail_after(0, || source.try_grow()),
            ] {
                assert!(matches!(
                    result,
                    Err(reserve::Error::Resident(resident::AllocError::Exhausted(_)))
                ));
                assert_eq!(source.resident(), 0);
                assert_eq!(budget.resident(), 1);
                empty(&mut source, &budget);
            }
            source.try_reserve(4).expect("staging reservation fits");
            assert_eq!(source.resident(), 4);
            empty(&mut source, &budget);
            source.write_slice(b"efgh").expect("staging append fits");
            assert_eq!(source.as_slice(), b"efgh");
        })
        .expect("resident test domain must admit its budget");
}
