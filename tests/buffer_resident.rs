pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{mem, num, pin::Pin};

use common::{alloc::system::Tracker, buffer::domain::Capacity};
use o3::{
    base::cell::region,
    buffer::{
        Residency as _,
        hold::{
            self,
            scope::{self, Input},
        },
        pool::{self, state},
        resident::{self, staging},
        storage,
        write::{Sink as _, SpareExt as _},
    },
};

#[test]
fn slices_use_one_alloc() {
    Capacity(6)
        .try_scope(|budget| {
            let (exact, allocs) = Tracker::measure(|| {
                resident::Exact::try_slices(&budget, [&b"pre"[..], &[], b"tag"])
            });
            let exact = exact.expect("six-byte resident owner");
            assert_eq!((allocs[0], allocs[2]), (1, 0));
            assert_eq!(exact.as_slice(), b"pretag");
            assert_eq!(exact.bytes(), 6);
            let exact = {
                let clone = exact.try_clone().expect("second exact edge");
                drop(exact);
                clone
            };
            let clone = {
                let shared = exact.into_shared();
                shared.try_clone().expect("second edge")
            };
            assert_eq!(budget.resident(), 6);
            let ((), freed) = Tracker::measure(move || assert_eq!(clone.as_slice(), b"pretag"));
            assert_eq!(freed, [0, 0, 1, allocs[1]]);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident domain");
}

#[test]
fn detaches_borrowed_pool() {
    let pool = (pool::Pool::<state::Uninit>::try_new(1, 16)).expect("fixture pool must allocate");
    let mut cursor = (pool.view()).expect("the only pool slot is vacant");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"borrowed"), b"borrowed".len());
        spare.commit();
    }
    let pointer = cursor.as_ref().as_ptr();
    let scoped = resident::Shared::from(cursor.freeze());

    let (detached, count) = Tracker::count(|| scoped.try_detach());
    match detached {
        Ok(shared) => {
            {
                let shared = shared;
                assert_eq!(count, 0);
                assert_eq!(shared.as_slice().as_ptr(), pointer);
                assert_eq!(shared.as_slice(), b"borrowed");
                assert_eq!(pool.available(), 0);
            }
            assert_eq!(pool.available(), 1);
        }
        outcome => {
            assert!(
                outcome.is_ok(),
                "plain pool ownership must detach without allocation"
            );
        }
    }
}

#[test]
fn transfers_unique() {
    let (owner, built) = Tracker::measure(|| {
        Capacity(8)
            .try_scope(|budget| {
                let mut owner = storage::Owned::try_allocate(&budget, 8).expect("resident owner");
                let mut spare = owner.spare();
                spare.write_slice(b"body").expect("initialized prefix");
                spare.commit();
                let owner = owner.freeze().project(1..3).expect("projected extent");
                let pointer = owner.as_ptr();
                let sibling = owner.try_clone().expect("shared edge");
                let (owner, activity) = Tracker::measure(|| owner.try_owned());
                let owner = owner.expect_err("a shared resident stays bound");
                assert_eq!(activity, [0; 4]);
                assert_eq!(budget.resident(), 8);
                assert_eq!(owner.as_ptr(), pointer);
                drop(sibling);
                let (owner, activity) = Tracker::measure(|| owner.try_owned());
                let owner = owner.expect("sole resident transfers");
                assert_eq!(activity, [0; 4]);
                assert_eq!(budget.resident(), 0);
                assert_eq!(owner.as_ptr(), pointer);
                assert_eq!(owner.as_slice(), b"od");
                owner
            })
            .expect("resident domain")
    });
    assert_eq!((built[0], built[2]), (1, 0));
    let sibling = owner.try_clone().expect("independent shared edge");
    drop(owner);
    assert_eq!(sibling.as_slice(), b"od");
    let ((), freed) = Tracker::measure(|| drop(sibling));
    assert_eq!(freed, [0, 0, 1, built[1]]);
}

#[test]
fn retains_independent_view() {
    let pool = pool::Pool::<state::Uninit>::try_new(1, 16).expect("pool");
    let mut cursor = pool.view().expect("slot");
    {
        let mut spare = cursor.spare();
        spare.write(b"borrowed");
        spare.commit();
    }
    let source = resident::Shared::from(cursor.freeze());
    let view = source.get(1..4).expect("range");
    let pointer = view.as_slice().as_ptr();
    let (retained, allocs) = Tracker::count(|| view.try_own());
    let retained = retained.expect("retain").expect("independent");
    drop(source);
    assert_eq!(allocs, 0);
    assert_eq!(retained.as_slice().as_ptr(), pointer);
    assert_eq!(retained.as_slice(), b"orr");
    assert_eq!(pool.available(), 0);
    drop(retained);
    assert_eq!(pool.available(), 1);

    let bytes = [1, 2];
    let scoped = resident::Shared::from(bytes.as_slice());
    assert!(
        scoped
            .get(0..1)
            .expect("range")
            .try_own()
            .expect("scope")
            .is_none()
    );
    assert!(
        scoped
            .get(0..0)
            .expect("empty")
            .try_own()
            .expect("empty")
            .expect("independent")
            .is_empty()
    );
}

#[test]
fn rejects_resident_pool() {
    Capacity(24)
        .try_scope(|budget| {
            let planned =
                staging::Plan::new(8, 8).and_then(|plan| staging::Layout::new(1, plan, 1, 0));
            let layout = (planned).expect("valid staging capacity");

            let pool = (staging::Pool::try_new(&budget, layout))
                .expect("the ownership staging pool must allocate");
            let mut window =
                (staging::Buf::try_from(&pool)).expect("the receive owner must be retained");
            window
                .write_slice(b"resident")
                .as_ref()
                .expect("fixture payload must fit");
            let pointer = window.as_slice().as_ptr();
            let shared = scope::Input::new(&window, &budget)
                .defer_all()
                .bind(hold::Policy::share())
                .try_into();
            let shared = (shared).expect("the received bytes must retain their accounted owner");

            let (detached, count) = Tracker::count(|| shared.try_detach());
            assert_eq!(count, 0);
            assert!(
                detached.is_err(),
                "resident ownership must reject zero-copy detachment"
            );
            let Err(shared) = detached else {
                return;
            };
            let (result, count) = Tracker::count(|| storage::Shared::try_copy(shared.as_slice()));
            let escaped = result.expect("resident bytes must copy into plain storage");

            assert_eq!(count, 1);
            assert_ne!(escaped.as_slice().as_ptr(), pointer);
            assert_eq!(escaped.as_slice(), b"resident");
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn rejects_reuse() {
    let mut domain = resident::Domain::new();
    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = (domain.try_budget(8, token.proof()))
            .expect("a fresh domain must admit its first generation");
        let charge = (budget.try_charge(8)).expect("the exact budget must be available");
        mem::forget(charge);
    });
    for capacity in [0, 4, 16] {
        region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
            let domain = domain.get_mut();
            let result = domain.try_budget(capacity, token.proof());
            assert!(
                result.is_err(),
                "a forgotten charge must prevent domain reuse"
            );
            let Err(cause) = result else { return };
            assert_eq!(
                cause,
                resident::Reuse {
                    retained: num::NonZero::<usize>::MIN.saturating_add(7),
                    previous: 8,
                    requested: capacity,
                }
            );
        });
    }
}

#[test]
fn rejects_growth() {
    let mut domain = resident::Domain::new();
    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = (domain.try_budget(10, token.proof()))
            .expect("a fresh domain must admit its first generation");
        {
            let mut first = (budget.try_charge(7)).expect("seven bytes must fit");
            {
                let second = (budget.try_charge(3)).expect("the remaining three bytes must fit");
                assert_eq!(second.capacity(), 3);
                let result = first.try_resize(8);
                assert!(
                    result.is_err(),
                    "an additional byte must exceed the full budget"
                );
                let Err(error) = result else { return };
                assert_eq!(error.into_parts(), (num::NonZero::<usize>::MIN, 0));
                assert_eq!((first.capacity(), budget.resident()), (7, 10));
            }
            assert_eq!(first.try_resize(9), Ok(()));
            assert_eq!((first.capacity(), budget.available()), (9, 1));
        }
        assert_eq!(budget.resident(), 0);
    });
    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = (domain.try_budget(0, token.proof()))
            .expect("released credit must permit a new empty generation");
        assert_eq!((budget.capacity(), budget.resident()), (0, 0));
    });
}

#[test]
fn admits_full_width() {
    let mut domain = resident::Domain::new();
    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = (domain.try_budget(usize::MAX, token.proof()))
            .expect("a fresh domain must accept the full usize limit");
        assert_eq!(budget.charge().capacity(), 0);
        assert_eq!(budget.resident(), 0);
        {
            let mut charge = (budget.try_charge(usize::MAX))
                .expect("the full limit must be chargeable without overflow");
            assert_eq!(budget.available(), 0);
            let empty = (budget.try_charge(0)).expect("a zero charge must require no capacity");
            assert_eq!(empty.capacity(), 0);
            let result = budget.try_charge(1);
            assert!(
                result.is_err(),
                "a full budget must reject an additional byte"
            );
            let Err(error) = result else { return };
            assert_eq!(error.into_parts(), (num::NonZero::<usize>::MIN, 0));
            assert_eq!(charge.try_resize(1), Ok(()));
            assert_eq!(budget.available(), usize::MAX - 1);
        }
        assert_eq!(budget.resident(), 0);
    });
}

#[test]
fn tracks_resident_owner() {
    let mut domain = resident::Domain::new();
    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = domain.try_budget(4, token.proof());
        let budget = budget.expect("asserted empty resident domain");
        let result = storage::Owned::try_allocate(&budget, 4);
        let mut bytes = result.expect("four resident bytes must fit their budget");
        let ptr = bytes.as_ptr();
        let mut spare = bytes.spare();
        spare
            .write_slice(b"data")
            .as_ref()
            .expect("required fixture outcome");
        spare.commit();
        {
            let clone = {
                let shared = bytes.freeze();
                let clone = shared.try_clone().expect("asserted retained edge");
                assert_eq!(shared.as_ptr(), ptr);
                assert_eq!(clone.as_ptr(), ptr);
                assert_eq!(budget.resident(), 4);
                let Err(resident::AllocError::Budget(error)) =
                    storage::Owned::try_allocate(&budget, 1)
                else {
                    panic!("allocation beyond the budget must fail at admission");
                };
                assert_eq!(error.into_parts(), (num::NonZero::<usize>::MIN, 0));
                clone
            };
            assert_eq!(clone.as_ptr(), ptr);
            assert_eq!(budget.resident(), 4);
        }
        assert_eq!(budget.resident(), 0);
    });
}

#[test]
fn response_retains_source() {
    use o3::buffer::hold;

    for account in [false, true] {
        for capacity in [4, 8] {
            Capacity(64)
                .try_scope(|budget| {
                    {
                        let (retained, pointer) = {
                            let plan = (staging::Plan::new(4, 8)).expect("valid staging capacity");

                            let layout = (staging::Layout::new(1, plan, 1, 8))
                                .expect("valid staging capacity");

                            let pool = if account {
                                staging::Pool::try_account(&budget, layout)
                            } else {
                                staging::Pool::try_new(&budget, layout)
                            }
                            .expect("response staging pool");
                            let mut source =
                                staging::Buf::try_from(&pool).expect("response staging buffer");
                            source.try_reserve(capacity).expect("warm or grown backing");
                            source.write_slice(b"body").expect("complete response");
                            let pointer = source.as_slice().as_ptr();
                            let (retained, activity) = Tracker::measure(|| {
                                Input::new(&source, &budget)
                                    .defer_all()
                                    .bind(hold::Policy::share())
                                    .try_into()
                            });
                            let retained = retained.expect("retain exact response source");
                            assert_eq!(activity, [0; 4]);
                            assert_eq!(retained.as_slice().as_ptr(), pointer);
                            source.clear();
                            (retained, pointer)
                        };
                        assert_eq!(retained.as_slice(), b"body");
                        assert_eq!(retained.as_slice().as_ptr(), pointer);
                        assert_eq!(retained.resident(), capacity);
                    }
                    assert_eq!(budget.resident(), 0);
                })
                .expect("resident test domain must admit its budget");
        }
    }
}

#[test]
fn reuses_resident_domain() {
    let mut domain = resident::Domain::new();

    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = domain.try_budget(8, token.proof());
        let budget = budget.expect("asserted empty resident domain");
        let charge = (budget.try_charge(1)).expect("one byte must fit the eight-byte budget");
        assert_eq!((charge.capacity(), budget.resident()), (1, 1));
    });

    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let domain = domain.get_mut();
        let budget = domain.try_budget(13, token.proof());
        let budget = budget.expect("asserted empty resident domain");
        assert_eq!(budget.capacity(), 13);
    });
}
