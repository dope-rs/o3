pub mod common;

use std::num::NonZero;

use common::{
    alloc::system::{Activity, Tracker},
    buffer::{Local, domain::Capacity},
};
use o3::buffer::{
    Residency as _,
    hold::{
        self,
        scope::{self, Input},
    },
    resident::{
        self,
        staging::{self, reserve},
    },
    write::{Sink as _, consume::Consume as _},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn accounted_geometry() {
    Capacity(4096)
        .try_scope(|budget| {
            let planned =
                staging::Plan::new(17, 17).and_then(|plan| staging::Layout::new(1, plan, 2, 0));
            let layout = (planned).expect("valid staging capacity");
            let last = {
                let retained;
                let bytes;
                let owner = {
                    let mut window;
                    let closed = {
                        let (created, build) =
                            Tracker::activity(|| staging::Pool::try_new(&budget, layout));
                        let pool = (created).expect("accounted staging allocation");
                        assert_eq!(build.allocations, 2);
                        window = (staging::Buf::try_from(&pool))
                            .expect("the receive owner must be retained");
                        window
                            .write_slice(b"retained")
                            .as_ref()
                            .expect("required fixture outcome");
                        bytes = (32 + 3 * 16 + 24 + 2 * 32 + 3 * 17_usize).next_multiple_of(8);
                        assert_eq!(build.allocated_bytes, bytes + 32);
                        assert_eq!(budget.resident(), 51);
                        let (held, hot) = Tracker::activity(|| {
                            scope::Input::new(&window, &budget)
                                .defer_all()
                                .bind(hold::Policy::share())
                                .try_into()
                        });
                        retained = (held).expect("the account must retain its original slot");
                        assert!(hot == Activity::default());
                        assert_eq!(retained.as_slice().as_ptr(), window.as_slice().as_ptr());
                        assert_eq!(budget.resident(), 68);
                        Tracker::begin()
                    };
                    assert!(closed.finish() == Activity::default());
                    Tracker::begin()
                };
                let owner = owner.finish();
                assert_eq!(owner.allocations, 0);
                assert_eq!(owner.deallocations, 1);
                assert_eq!(owner.deallocated_bytes, 32);
                assert_eq!(retained.as_slice(), b"retained");
                assert_eq!(budget.resident(), 68);
                (Tracker::begin(), bytes)
            };
            let (last, bytes) = last;
            let last = last.finish();
            assert_eq!(last.allocations, 0);
            assert_eq!(last.deallocations, 1);
            assert_eq!(last.deallocated_bytes, bytes);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn preserves_numeric_bounds() {
    let max = u32::MAX as usize;
    for (warm, limit) in [(1, 1), (1, max), (max - 1, max), (max, max)] {
        let (created, counts) = Tracker::measure(|| staging::Plan::new(warm, limit));
        let plan = (created).expect("valid staging capacity");

        assert_eq!(counts, [0; 4]);
        assert_eq!(plan.warm().get() as usize, warm);
        assert_eq!(plan.ceiling().get() as usize, limit);
    }
    assert!(matches!(
        staging::Plan::new(0, 0),
        Err(staging::Error::ZeroWarm)
    ));
    for (warm, limit) in [(1, 0), (2, 1)] {
        let rejected = staging::Plan::new(warm, limit);
        assert!(matches!(rejected,
            Err(staging::Error::WarmExceedsLimit { warm: actual, limit: ceiling })
                if actual.get() == warm && ceiling == limit));
    }
    if let Some(large) = max.checked_add(1) {
        let rejected = staging::Plan::new(large, large);
        assert!(
            matches!(rejected, Err(staging::Error::LimitTooLarge { limit }) if limit.get() == large)
        );
    }
}

#[test]
fn preserves_growth() {
    let plan = (staging::Plan::new(3, 13)).expect("valid staging capacity");

    let layout = (staging::Layout::new(1, plan, 0, 13)).expect("valid staging capacity");

    for (shared, grow) in [(false, false), (false, true), (true, false), (true, true)] {
        Capacity(layout.resident().get())
            .try_scope(|budget| {
                let created = if shared {
                    staging::Pool::try_account(&budget, layout)
                } else {
                    staging::Pool::try_new(&budget, layout)
                };
                let pool = (created).expect("staging pool allocation");
                let mut buf = (staging::Buf::try_from(&pool)).expect("staging pool reference");
                let (rejected, activity) = Tracker::measure(|| {
                    buf.try_reserve(0).expect("empty reservation");
                    buf.write_slice(b"").expect("empty append");
                    buf.try_reserve(14)
                });
                assert!(matches!(rejected, Err(reserve::Error::Capacity(error))
                    if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(13), 13)));
                assert_eq!(activity, [0; 4]);
                assert_eq!(buf.resident(), 0);
                let failed = Tracker::fail_after(0, || buf.try_reserve(13));
                assert!(matches!(
                    failed,
                    Err(reserve::Error::Resident(resident::AllocError::Exhausted(_)))
                ));
                assert_eq!(buf.resident(), 0);
                assert_eq!(budget.resident(), 3);

                let ((), hot) = Tracker::measure(|| {
                    for _ in 0..32 {
                        buf.try_grow().expect("staging growth fits");
                        buf.write_slice(b"abc").expect("staging append fits");
                        assert_eq!(buf.as_slice(), b"abc");
                        assert_eq!(buf.consume_max(3), 3);
                    }
                });
                assert_eq!(hot, [0; 4]);

                for (len, capacity) in [(1, 3), (3, 3), (4, 6), (6, 6), (9, 12), (12, 12), (13, 13)]
                {
                    if grow {
                        let pointer = buf.as_slice().as_ptr();
                        let charge = budget.resident();
                        let spare = buf.spare();
                        buf.try_grow()
                            .expect("staging growth reaches the next capacity");
                        if spare != 0 {
                            assert_eq!(buf.as_slice().as_ptr(), pointer);
                            assert_eq!(budget.resident(), charge);
                        }
                    }
                    buf.write_slice(&b"abcdefghijklm"[buf.len()..len])
                        .expect("staging append fits");
                    assert_eq!(buf.as_slice(), &b"abcdefghijklm"[..len]);
                    assert_eq!(buf.resident(), capacity);
                }
                let pointer = buf.as_slice().as_ptr();
                let ((), rejected) = Tracker::measure(|| {
                    for (result, need, room) in [
                        (buf.try_grow(), 1, 0),
                        (buf.write_slice(b"b"), 1, 0),
                        (buf.try_reserve(usize::MAX), usize::MAX, 13),
                    ] {
                        assert!(matches!(result, Err(reserve::Error::Capacity(_))));
                        let Err(reserve::Error::Capacity(error)) = result else {
                            return;
                        };
                        let (required, free) = error.into_parts();
                        assert_eq!((required.get(), free), (need, room));
                    }
                    buf.write_slice(b"").expect("staging append fits");
                    buf.try_reserve(0).expect("staging reservation fits");
                    buf.try_reserve(13).expect("staging reservation fits");
                });
                assert_eq!(rejected, [0; 4]);
                assert_eq!(buf.as_slice().as_ptr(), pointer);
                assert_eq!(buf.as_slice(), b"abcdefghijklm");
                let charge = budget.resident();
                assert_eq!(buf.consume_max(1), 1);
                let ((), reused) = Tracker::measure(|| {
                    buf.try_grow().expect("staging growth fits");
                    buf.write_slice(b"z").expect("staging append fits");
                });
                assert_eq!(reused, [0; 4]);
                assert_eq!(buf.as_slice(), b"bcdefghijklmz");
                assert_eq!(buf.as_slice().as_ptr(), pointer);
                assert_eq!(budget.resident(), charge);
                buf.clear();
                assert_eq!(buf.resident(), 0);
                assert_eq!(budget.resident(), 3);
            })
            .expect("resident test domain must admit its budget");
    }
}

#[test]
fn growth_retries() {
    for (shared, denied) in [(false, false), (false, true), (true, false), (true, true)] {
        let fixture = Local::try_new().expect("local staging geometry");
        Capacity(fixture.0.resident().get())
            .try_scope(|budget| {
                let mut buf = fixture.try_buf(&budget).expect("local staging fixture");
                {
                    let held = if shared {
                        let retained = Input::new(&buf, &budget)
                            .defer_all()
                            .bind(hold::Policy::share())
                            .try_into();
                        let held = (retained).expect("local backing retention");
                        Some(held)
                    } else {
                        None
                    };
                    let pointer = buf.as_slice().as_ptr();
                    let required = if shared { 5 } else { 1 };
                    let charge = budget
                        .try_charge(if denied {
                            budget.available() - required
                        } else {
                            0
                        })
                        .expect("leave the exact growth budget");
                    let ((result, activity), resizes) = Tracker::resizes(|| {
                        Tracker::measure(|| {
                            if denied {
                                buf.write_slice(b"e")
                            } else {
                                Tracker::fail_after(0, || buf.write_slice(b"e"))
                            }
                        })
                    });
                    result.expect("preferred growth must retry its exact capacity");
                    drop(charge);
                    assert_eq!((activity[0], activity[2]), (1, usize::from(!shared)));
                    assert_eq!(resizes, usize::from(!shared));
                    assert_eq!(buf.as_slice(), b"abcde");
                    assert_eq!(buf.resident(), 5);
                    assert_eq!(budget.resident(), 6 + usize::from(shared) * 4);
                    if let Some(held) = held {
                        assert_eq!(held.as_slice(), b"abcd");
                        assert_eq!(held.as_slice().as_ptr(), pointer);
                        assert_ne!(buf.as_slice().as_ptr(), pointer);
                        assert_eq!(buf.consume_max(5), 5);
                        assert_eq!(buf.resident(), 0);
                        assert_eq!(budget.resident(), 5);
                    }
                }
                if shared {
                    assert_eq!(budget.resident(), 1);
                }
            })
            .expect("resident test domain must admit its budget");
    }
}

#[test]
fn growth_preserves_failure() {
    for shared in [false, true] {
        for denied in [false, true] {
            let fixture = Local::try_new().expect("local staging geometry");
            Capacity(fixture.0.resident().get())
                .try_scope(|budget| {
                    let mut buf = fixture.try_buf(&budget).expect("local staging fixture");
                    let held = if shared {
                        let retained = Input::new(&buf, &budget)
                            .defer_all()
                            .bind(hold::Policy::share())
                            .try_into();
                        let held = (retained).expect("local backing retention");
                        Some(held)
                    } else {
                        None
                    };
                    let pointer = buf.as_slice().as_ptr();
                    {
                        let blocker = if denied {
                            let blocker = (resident::Exact::try_copy(&budget, &[b'x'; 14]))
                                .expect("leave exactly two resident bytes");
                            Some(blocker)
                        } else {
                            None
                        };
                        if let Some(blocker) = &blocker {
                            assert_eq!(blocker.as_slice(), &[b'x'; 14]);
                        }
                        let before = budget.resident();
                        let (result, activity) = Tracker::measure(|| {
                            Tracker::fail_after(0, || buf.write_slice(b"efgh"))
                        });

                        assert!(
                            result.is_err(),
                            "failed local growth must preserve its resident cause"
                        );
                        let Err(error) = result else { return };
                        let (error,) = match error {
                            reserve::Error::Resident(error) => Some((error,)),
                            _ => None,
                        }
                        .expect("failed local growth must preserve its resident cause");
                        {
                            match (denied, error) {
                                (true, resident::AllocError::Budget(error)) => {
                                    let (need, free) = error.into_parts();
                                    assert_eq!((need.get(), free), (if shared { 8 } else { 4 }, 2));
                                    Some(())
                                }
                                (false, resident::AllocError::Exhausted(_)) => Some(()),
                                _ => None,
                            }
                            .expect("growth must preserve its exact failure variant")
                        }
                        assert_eq!(activity, [0; 4]);
                        assert_eq!(budget.resident(), before);
                        assert_eq!(buf.as_slice(), b"abcd");
                        assert_eq!(buf.as_slice().as_ptr(), pointer);
                        assert_eq!(buf.resident(), 4);
                    }
                    buf.write_slice(b"efgh").expect("staging append fits");
                    assert_eq!(buf.as_slice(), b"abcdefgh");
                    if let Some(held) = held {
                        assert_eq!(held.as_slice(), b"abcd");
                        assert_eq!(held.as_slice().as_ptr(), pointer);
                    }
                })
                .expect("resident test domain must admit its budget");
        }
    }
    Capacity(4096)
        .try_scope(|budget| {
            let planned =
                staging::Plan::new(17, 17).and_then(|plan| staging::Layout::new(1, plan, 1, 0));
            let layout = (planned).expect("valid staging capacity");

            let (failed, activity) = Tracker::activity(|| {
                Tracker::fail_after(0, || staging::Pool::try_new(&budget, layout))
            });
            assert!(matches!(failed, Err(resident::AllocError::Exhausted(_))));
            assert!(activity == Activity::default());
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn preserves_vectored_tail() {
    for (account, capacity, expected) in [
        (false, 4, &b"bchi"[..]),
        (false, 8, &b"bcdefghi"[..]),
        (true, 8, &b"bcdefghi"[..]),
    ] {
        Capacity(64)
            .try_scope(|budget| {
                let plan = staging::Plan::new(4, 8).expect("staging capacity");
                let layout = staging::Layout::new(1, plan, 1, 8).expect("staging layout");
                let pool = if account {
                    staging::Pool::try_account(&budget, layout)
                } else {
                    staging::Pool::try_new(&budget, layout)
                }
                .expect("staging pool");
                let mut source = staging::Buf::try_from(&pool).expect("staging owner");
                source.try_reserve(capacity).expect("initial capacity");
                let original = &b"abcdefg"[..capacity - 1];
                source.write_slice(original).expect("initial bytes");
                let mut occupied = staging::Buf::try_from(&pool).expect("spare warm owner");
                occupied
                    .write_slice(b"x")
                    .expect("occupy warm rotation slot");
                let held = Input::new(&source, &budget)
                    .defer_all()
                    .bind(hold::Policy::share())
                    .try_into()
                    .expect("shared backing");
                source.try_consume(1).expect("consumed prefix");
                let pointer = source.as_slice().as_ptr();
                assert!(matches!(
                    source.write_slices([&b"h"[..], b"ijklmnop"]),
                    Err(reserve::Error::Capacity(_))
                ));
                assert_eq!(source.as_slice(), &original[1..]);
                assert_eq!(held.as_slice(), original);
                {
                    let blocker = budget
                        .try_charge(budget.available())
                        .expect("resident blocker");
                    let result = source.write_slices([&b"h"[..], b"i"]);
                    assert!(
                        matches!(
                            result,
                            Err(reserve::Error::Resident(resident::AllocError::Budget(_)))
                                | Err(reserve::Error::Spill {
                                    error: resident::AllocError::Budget(_),
                                    ..
                                })
                        ),
                        "account={account}, capacity={capacity}, result={result:?}"
                    );
                    assert_eq!(source.as_slice(), &original[1..]);
                    assert_eq!(held.as_slice(), original);
                    drop(blocker);
                }
                let result = Tracker::fail_after(0, || source.write_slices([&b"h"[..], b"i"]));
                assert!(matches!(
                    result,
                    Err(reserve::Error::Resident(resident::AllocError::Exhausted(_)))
                        | Err(reserve::Error::Spill {
                            error: resident::AllocError::Exhausted(_),
                            ..
                        })
                ));
                assert_eq!(source.as_slice(), &original[1..]);
                assert_eq!(source.as_slice().as_ptr(), pointer);
                assert_eq!(held.as_slice(), original);
                source
                    .write_slices([&b"h"[..], b"i"])
                    .expect("complete retry");
                assert_eq!(source.as_slice(), expected);
                assert_eq!(held.as_slice(), original);
                assert_eq!(occupied.as_slice(), b"x");
            })
            .expect("resident domain");
    }
}
