pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell, pin::Pin};

use common::{
    alloc::system::{Activity, Tracker},
    buffer::domain::Capacity,
};
use o3::{
    base::cell::region,
    buffer::{
        Residency as _,
        hold::{
            self,
            scope::{self, Input},
        },
        resident::{self, staging},
        storage,
        write::{Sink as _, SpareExt as _},
    },
};

#[test]
fn materializes_policy_choices() {
    Capacity(64)
        .try_scope(|budget| {
            let budget = &budget;
            let mut source = (storage::Owned::try_with(8)).expect("the source allocation must fit");
            let mut spare = source.spare();
            spare
                .write_slice(b"payload")
                .expect("the payload must fit the source allocation");
            spare.commit();
            let snap = source.freeze();
            let want = snap.as_slice().as_ptr().wrapping_add(1);

            let sample = Tracker::begin();
            let input = scope::Input::new(&snap, budget);
            let result = input.try_plan(7..9);
            assert!(result.is_err(), "range exceeds source");
            let Err(error) = result else { return };
            assert_eq!((error.start(), error.end(), error.len()), (7, 9, 7));
            let plan = (input.try_plan(1..6)).expect("the snapshot range must be in bounds");
            let bound = plan.bind(hold::Policy::new(1, 2));
            assert_eq!(bound.ceiling(), 8);
            let bytes = bound.as_slice();
            let shared = bound.try_into();
            assert!(sample.finish() == Activity::default());
            let shared = shared.expect("the bounded view must fit its sharing policy");
            assert_eq!(bytes, b"ayloa");
            assert_eq!(shared.as_slice(), bytes);
            assert_eq!(shared.as_slice().as_ptr(), want);

            let (copied, alloc) = Tracker::measure(|| {
                let bound = scope::Input::new(&snap, budget)
                    .defer_all()
                    .bind(hold::Policy::compact());
                assert_eq!(bound.ceiling(), 7);
                bound.try_into()
            });
            assert_eq!(alloc[0], 1);
            assert_eq!(alloc[2], 0);
            let copied = copied.expect("the compact copy must fit its resident budget");
            assert_eq!(copied.as_slice(), b"payload");
            assert_ne!(copied.as_slice().as_ptr(), snap.as_slice().as_ptr());
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn policy_bounds() {
    use hold::limits;

    let policy = hold::Policy::new(4 * 1024, 2);

    assert!(
        (policy
            .try_limit(4 * 1024)
            .expect("buffer transition accepted")
            .bytes())
            == (8 * 1024)
    );
    assert!(
        (policy
            .try_limit(4 * 1024 - 1)
            .expect("buffer transition accepted")
            .bytes())
            == (4 * 1024 - 1)
    );
    assert!(
        policy.try_limit(usize::MAX)
            == Err(limits::Error::Scale {
                logical: usize::MAX,
                factor: 2,
            })
    );
    assert!(
        (hold::Policy::new(1, 0)
            .try_limit(7)
            .expect("buffer transition accepted")
            .bytes())
            == (7)
    );
}

#[test]
fn skips_irrelevant_charge() {
    use hold::{Choice, Policy};

    let policy = Policy::new(4 * 1024, 2);
    let calls = cell::Cell::new(0);
    for (policy, logical, choice) in [
        (policy, 0, Choice::Empty),
        (policy, 4 * 1024 - 1, Choice::Copy),
        (Policy::compact(), usize::MAX, Choice::Copy),
        (Policy::new(1, 0), 7, Choice::Copy),
        (Policy::share(), 1, Choice::Share),
        (Policy::new(0, 1), usize::MAX, Choice::Share),
        (Policy::new(0, 2), usize::MAX, Choice::Share),
    ] {
        assert!(
            policy.choose(logical, || {
                calls.set(calls.get() + 1);
                usize::MAX
            }) == choice
        );
    }
    assert_eq!(calls.get(), 0);
    for (resident, choice) in [(8192, Choice::Share), (8193, Choice::Copy)] {
        calls.set(0);
        assert!(
            policy.choose(4096, || {
                calls.set(calls.get() + 1);
                resident
            }) == choice
        );
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn limit_arithmetic() {
    use hold::limits::{Bound, Error};

    let bound = Bound::new(7);
    assert_eq!(bound.max(9).bytes(), 9);
    assert!(
        (bound
            .try_add(5)
            .expect("buffer transition accepted")
            .bytes())
            == (12)
    );
    assert!(
        (bound
            .try_mul(5)
            .expect("buffer transition accepted")
            .bytes())
            == (35)
    );
    assert!(
        Bound::new(usize::MAX).try_add(1)
            == Err(Error::Add {
                left: Bound::new(usize::MAX),
                right: 1,
            })
    );
    assert!(
        Bound::new(usize::MAX).try_mul(2)
            == Err(Error::Multiply {
                left: Bound::new(usize::MAX),
                right: 2,
            })
    );
}

#[test]
fn external_budget_admits() {
    let mut domains = [resident::Domain::new(), resident::Domain::new()];
    region::Token::scope_pinned(Pin::new(&mut domains), |token, domains| {
        let domains = domains.get_mut();
        let [origin, external] = domains;
        let origin = (origin.try_budget(0, token.proof()))
            .expect("the original resident domain must be empty");
        let external = (external.try_budget(8, token.proof()))
            .expect("the external resident domain must be empty");
        let source = (storage::Shared::try_copy(b"one-four"))
            .expect("the external-budget source must allocate");
        let pointer = source.as_slice().as_ptr();
        let bound = scope::Input::new(&source, &origin)
            .defer_all()
            .bind(hold::Policy::compact());

        let (shared, alloc) = Tracker::measure(|| bound.try_admit(&external));

        assert_eq!(alloc[0], 1);
        {
            let shared = shared
                .map_err(|(_, cause)| cause)
                .expect("the external budget must admit the compact copy");
            assert_eq!(shared.as_slice(), b"one-four");
            assert_ne!(shared.as_slice().as_ptr(), pointer);
            assert_eq!(origin.resident(), 0);
            assert_eq!(external.resident(), 8);
            assert_eq!(shared.resident(), 8);
        }
        assert_eq!(external.resident(), 0);
    });
}

#[test]
fn cursor_charges_budget() {
    Capacity(8)
        .try_scope(|budget| {
            let mut source = Vec::with_capacity(64);
            source.extend_from_slice(b"abcdefgh");
            let bytes = (storage::Shared::try_from(&mut source))
                .expect("vector sharing must allocate metadata");
            let source = bytes.as_slice().as_ptr().wrapping_add(2);
            let plan = (Input::new(&bytes, &budget).try_plan(2..6))
                .expect("the bytes range must be in bounds");

            let act = {
                let (retained, act) =
                    Tracker::measure(|| plan.bind(hold::Policy::compact()).try_into());
                let shared = retained.expect("deferred view must fit the resident budget");
                assert_eq!(shared.resident(), 4);
                assert_eq!(budget.resident(), 4);
                assert_eq!(shared.as_slice(), b"cdef");
                assert_ne!(shared.as_slice().as_ptr(), source);
                act
            };
            assert_eq!(act[0], 1);
            assert_eq!(act[2], 0);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn returns_inputs() {
    Capacity(0)
        .try_scope(|budget| {
            let budget = &budget;
            let source = (storage::Shared::try_copy(b"one-four"))
                .expect("the rejection source must allocate");
            let input = scope::Input::new(&source, budget).defer_all();

            let retained = input.bind(hold::Policy::compact()).try_retain();
            let result = retained;
            assert!(result.is_err(), "zero budget must reject materialization");
            let Err((bound, error)) = result else { return };
            let error = match error {
                hold::Error::Resident(resident::AllocError::Budget(error)) => Some(error),
                _ => None,
            }
            .expect("zero budget must preserve the exact budget failure");
            let (required, free) = error.into_parts();
            assert_eq!(required.get(), 8);
            assert_eq!(free, 0);
            let input = bound.defer();
            assert_eq!(input.as_slice(), b"one-four");
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn retries_staging() {
    for shared in 0..=1 {
        Capacity(32)
            .try_scope(|budget| {
                let budget = &budget;
                let planned = staging::Plan::new(8, 8)
                    .and_then(|plan| staging::Layout::new(2 - shared, plan, shared, 0));
                let layout = (planned).expect("valid staging capacity");

                let pool = (staging::Pool::try_new(budget, layout))
                    .expect("the retry staging pool must allocate");
                let mut window =
                    (staging::Buf::try_from(&pool)).expect("the receive owner must be retained");
                window
                    .write_slice(b"one-four")
                    .expect("the payload must fit the staging window");
                let pointer = window.as_slice().as_ptr().wrapping_add(2);
                let denied = {
                    let blocker = (resident::Exact::try_copy(budget, &[b'r'; 16]))
                        .expect("the remaining sixteen bytes must allocate");
                    let plan = (scope::Input::new(&window, budget).try_plan(2..6))
                        .expect("the four-byte range must fit");
                    let (denied, ops) =
                        Tracker::measure(|| plan.bind(hold::Policy::share()).try_retain());
                    assert_eq!(ops, [0; 4]);
                    assert_eq!(blocker.as_slice(), &[b'r'; 16]);
                    assert_eq!(budget.resident(), 32);
                    denied
                };
                let result = denied;
                assert!(
                    result.is_err(),
                    "retention must fail while the budget is occupied"
                );
                let Err((bound, error)) = result else { return };
                let error = match error {
                    hold::Error::Resident(resident::AllocError::Budget(error)) => Some(error),
                    _ => None,
                }
                .expect("retention must preserve the exact budget failure");
                let (need, spare) = error.into_parts();
                assert_eq!(need.get(), if shared == 0 { 4 } else { 8 });
                assert_eq!(spare, 0);
                assert_eq!(budget.resident(), 16);
                assert_eq!(bound.ceiling(), 8);
                {
                    let (retained, ops) = Tracker::measure(|| bound.try_retain());
                    match retained {
                        Ok(retained) => {
                            assert_eq!(retained.as_slice(), b"e-fo");
                            assert_eq!(retained.as_slice().as_ptr() == pointer, shared == 1);
                            assert_eq!(ops[0], usize::from(shared == 0));
                            assert_eq!(ops[2], 0);
                            assert_eq!(budget.resident(), 16 + need.get());
                        }
                        outcome => assert!(
                            outcome.is_ok(),
                            "retention rejected: {:?}",
                            outcome.as_ref().err().map(|(_, cause)| cause),
                        ),
                    }
                }
                assert_eq!(budget.resident(), 16);
            })
            .expect("resident test domain must admit its budget");
    }
}
