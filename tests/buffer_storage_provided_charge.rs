pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, num::NonZero, pin::Pin};

use common::{
    alloc::system::Tracker,
    buffer::{
        domain::Capacity,
        provided::{
            Item,
            scope::{Context, Phase, Reuse, Terminal},
        },
    },
};
use o3::{
    alloc,
    buffer::{
        self, Complete as _,
        hold::{self, scope},
        resident,
        storage::{
            self,
            provided::{self, Frozen as _, Mutable as _, account},
        },
    },
};

#[test]
fn prefixes_preserve_owner() {
    use buffer::raw::Freeze as _;

    Item::try_scope(|unique, budget, arena| {
        let held = {
            let mut unique = unique;
            let three = NonZero::<usize>::MIN.saturating_add(2);
            let eight = NonZero::<usize>::MIN.saturating_add(7);
            assert!(
                matches!(unique.try_take(eight), Err(error) if error.into_parts() == (eight, 7))
            );
            assert_eq!(unique.as_slice(), b"exactly");
            assert_eq!(budget.resident(), 0);
            assert!(!arena.has_returned());

            let mut prefix = unique.try_take(three).expect("three-byte prefix must fit");
            prefix.as_mut()[0] = b'E';
            let pointer = prefix.as_ref().as_ptr();
            let prefix = prefix.freeze();
            assert_eq!(prefix.as_ref().as_ptr(), pointer);
            {
                let charge = budget.try_charge(1).expect("one-byte charge must fit");
                assert!(
                    matches!(prefix.try_account(&budget), Err(account::Cause::Budget(error))
            if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(6), 6))
                );
                assert_eq!(budget.resident(), charge.capacity());
                assert_eq!(prefix.as_ref(), b"Exa");
            }
            let held = prefix
                .try_account(&budget)
                .expect("prefix must retain its owner");
            assert_eq!(unique.as_slice(), b"ctly");
            unique.as_mut_slice()[0] = b'C';
            assert_eq!(held.as_slice(), b"Exa");
            assert_eq!(unique.as_slice(), b"Ctly");
            assert_eq!(budget.resident(), 7);
            held
        };
        assert!(!arena.has_returned());
        assert_eq!(held.as_slice(), b"Exa");
    })
    .expect("provided charge fixture");
}

#[test]
fn chunks_preserve_owner() {
    use buffer::raw::Freeze as _;

    Item::try_scope(|unique, budget, arena| {
        let held = {
            let mut unique = unique;
            let start = unique.as_slice().as_ptr();
            let size = NonZero::<usize>::MIN.saturating_add(2);
            let held = {
                let mut chunks = provided::Chunks::new(&mut unique, size);
                let mut first = chunks.next().expect("first segment must exist");
                let mut second = chunks.next().expect("second segment must exist");
                let last = chunks.next().expect("final partial segment must exist");
                assert!(chunks.next().is_none());
                assert_eq!(first.as_ref(), b"exa");
                assert_eq!(second.as_ref(), b"ctl");
                assert_eq!(last.as_ref(), b"y");
                assert_eq!(first.as_ref().as_ptr(), start);
                assert_eq!(second.as_ref().as_ptr(), start.wrapping_add(3));
                assert_eq!(last.as_ref().as_ptr(), start.wrapping_add(6));
                let four = size.saturating_add(1);
                assert!(
                    matches!(first.try_take(four), Err(error) if error.into_parts() == (four, 3))
                );
                assert_eq!(first.as_ref(), b"exa");
                let mut prefix = first
                    .try_take(NonZero::<usize>::MIN)
                    .expect("one-byte nested prefix must fit");
                prefix.as_mut()[0] = b'E';
                second.as_mut()[0] = b'C';
                assert_eq!(prefix.as_ref(), b"E");
                assert_eq!(first.as_ref(), b"xa");
                assert_eq!(second.as_ref(), b"Ctl");
                assert_eq!(budget.resident(), 0);
                assert!(!arena.has_returned());
                let frozen = prefix.freeze();
                assert_eq!(frozen.as_ref().as_ptr(), start);
                let held = frozen
                    .try_account(&budget)
                    .expect("nested prefix must retain its original owner");
                assert_eq!(budget.resident(), 7);
                held
            };
            assert!(unique.as_slice().is_empty());
            assert!(unique.as_mut_slice().is_empty());
            held
        };
        assert_eq!(held.as_slice(), b"E");
        assert!(!arena.has_returned());
    })
    .expect("provided charge fixture");
}

#[test]
fn keeps_foreign_charge() {
    let mut arena =
        provided::Arena::<_, _>::try_with(NonZero::<u32>::MIN, || Ok::<(), alloc::Error>(()))
            .expect("one-slot arena must allocate");
    let mut domains = (resident::Domain::new(), resident::Domain::new());
    arena.scope(Pin::new(&mut domains), |token, arena, domains| {
        let (first, second) = domains.get_mut();
        let first = first
            .try_budget(1, token.proof())
            .expect("first empty domain must accept its budget");
        let second = second
            .try_budget(1, token.proof())
            .expect("second empty domain must accept its budget");
        {
            let owner = {
                let slot = arena.vacant().expect("initial vacancy must exist");
                let source = slot.try_insert([7], 1, |bytes| bytes.as_mut_slice());
                let source = source.expect("buffer transition accepted");

                let source = source.into_shared();
                let deferred = {
                    let (result, ops) = Tracker::measure(|| {
                        (source.try_account(&first), source.try_account(&second))
                    });
                    assert_eq!(ops, [0; 4]);
                    let owner = result.0.expect("first budget must admit its owner");
                    assert_eq!(owner.as_slice(), &[7]);
                    assert!(matches!(result.1, Err(account::Cause::Foreign)));
                    let input = scope::Input::new(&source, &second);
                    let bound = input.defer_all().bind(hold::Policy::share());
                    let (result, ops) = Tracker::measure(|| bound.try_retain());
                    assert_eq!(ops, [0; 4]);
                    let (bound, cause) = result
                        .expect_err("foreign retention must preserve its exact cause and input");
                    std::assert_matches!(cause, hold::Error::Account(account::Cause::Foreign));
                    let deferred = bound.defer();
                    assert_eq!(deferred.as_slice(), &[7]);
                    assert_eq!((first.resident(), second.resident()), (1, 0));
                    deferred
                };
                assert_eq!(first.resident(), 0);
                let (retry, ops) =
                    Tracker::measure(|| deferred.bind(hold::Policy::share()).try_retain());
                assert_eq!(ops, [0; 4]);
                let owner = retry
                    .map_err(|rejected| rejected.1)
                    .expect("the retry must retain within the matching resident domain");
                assert_eq!(owner.as_slice().as_ptr(), source.as_slice().as_ptr());
                assert_eq!((first.resident(), second.resident()), (0, 1));
                owner
            };
            assert!(!arena.has_returned());
            assert_eq!(owner.as_slice(), &[7]);
        }
        assert_eq!(second.resident(), 0);
        assert!(matches!(arena.pop_returned(), Some([7])));
    });
}

#[test]
fn shares_proved_charge() {
    Item::try_scope(|unique, budget, arena| {
        let held = {
            let source = unique.into_shared();
            {
                let prefix = {
                    let held = source
                        .try_account(&budget)
                        .expect("shared owner must fit its budget");
                    {
                        let again = source
                            .try_account(&budget)
                            .expect("shared owner must reuse its charge");
                        assert_eq!(again.as_slice(), b"exactly");
                    }
                    assert_eq!(budget.resident(), 7);
                    let clone = held.try_clone().expect("retained edge must be clonable");
                    let view = buffer::View::new(&clone)
                        .try_get(0..3)
                        .expect("proved owner's prefix must be in range");
                    view.try_retain()
                        .expect("proved prefix must retain its owner")
                };
                assert_eq!(budget.resident(), 7);
                assert_eq!(prefix.as_slice(), b"exa");
                assert!(!arena.has_returned());
            }
            assert_eq!(budget.resident(), 0);
            assert!(!arena.has_returned());
            source
                .try_account(&budget)
                .expect("released owner must fit its budget")
        };
        assert!(!arena.has_returned());
        assert_eq!(budget.resident(), 7);
        assert_eq!(held.as_slice(), b"exactly");
    })
    .expect("provided charge fixture");
}

#[test]
fn backing_keeps_charge() {
    Item::try_scope(|unique, budget, arena| {
        let held = {
            let source = unique.into_shared();
            let held = source.try_account(&budget).expect("registration fits");
            let held = resident::Shared::from(held);
            let backing = held.backing().expect("registered owner has backing");
            assert!(source.try_account(&budget).is_ok());
            assert_eq!(backing.resident(), 7);
            let clone = held.try_clone().expect("registered owner must retain");
            let sibling = clone.backing().expect("registered clone has backing");
            assert!(backing.contains(&sibling) && sibling.contains(&backing));
            held
        };
        let backing = held.backing().expect("retained owner has backing");
        assert_eq!(backing.resident(), 7);
        assert_eq!(budget.resident(), 7);
        assert!(!arena.has_returned());
    })
    .expect("provided charge fixture");
}

#[test]
fn keeps_registration() {
    use buffer::{Read as _, Residency as _, bytes::bounded, write::consume::Consume as _};
    Item::try_scope(|unique, budget, arena| {
        let mut selected = {
            let source = bounded::Value::try_new(unique.into_shared());
            let source = source
                .map_err(|rejected| rejected.into_parts().1)
                .expect("buffer extent fits the signed count");
            let pointer = source.as_ref().as_ptr();
            let (head, _) = source.view().try_split::<2>().expect("two-byte header");
            assert_eq!(head, b"ex");
            assert_eq!(head.as_ptr(), pointer);
            let held = <bounded::Value<storage::Shared<provided::Owner<'_>>> as hold::raw::Retain<
                    '_,
                >>::try_retain(&source.view(), &budget)
                .expect("registered retention");
            assert_eq!(held.as_slice().as_ptr(), pointer);
            assert_eq!(held.resident(), 7);
            let clone = held.try_clone().expect("registered edge");
            let selected = clone.project(1..4);
            let selected = selected.expect("buffer transition accepted");
            assert_eq!(selected.as_slice(), b"xac");
            assert_eq!(selected.as_slice().as_ptr(), pointer.wrapping_add(1));
            selected
        };
        assert_eq!(budget.resident(), 7);
        assert!(!arena.has_returned());
        selected.try_consume(3).expect("complete selected range");
        assert_eq!(budget.resident(), 0);
        assert!(arena.has_returned());
    })
    .expect("provided charge fixture");
}

#[test]
fn reuses_last_registration() {
    let drops = Cell::new(Some(0));
    let mut domain = resident::Domain::new();
    let mut arena =
        provided::Arena::<_, _, ()>::try_allocate(NonZero::<u32>::MIN.saturating_add(1))
            .expect("two independent slots")(Context);
    let mut terminal = Terminal {
        phase: Phase::Active,
        items: [None, None],
        returned: 0,
        error: None,
    };
    let (rejected, resident, returned) = arena
        .run(
            Pin::new(&mut domain),
            1,
            &mut terminal,
            Reuse { drops: &drops },
        )
        .expect("registration remains charged until its last edge");
    assert_eq!(
        rejected,
        [provided::account::Cause::Exhausted { count: 1, limit: 1 }; 2]
    );
    assert_eq!(resident, [4, 0, 4, 0]);
    assert!(!returned, "unaccounted live owners retain their slots");
    assert!(terminal.error.is_none(), "{:?}", terminal.error);
    assert_eq!(terminal.returned, 2);
    drop(terminal);
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn rejects_without_refund() {
    use buffer::bytes::bounded;

    Item::try_scope(|unique, budget, arena| {
        let (held, pointer) = {
            let source = bounded::Value::try_new(unique.into_shared())
                .map_err(|rejected| rejected.into_parts().1)
                .expect("bounded registration");
            let held = <bounded::Value<storage::Shared<provided::Owner<'_>>> as hold::raw::Retain<
                    '_,
                >>::try_retain(&source.view(), &budget)
                .expect("registered retention");
            let pointer = held.as_slice().as_ptr();
            (held, pointer)
        };
        let result = held.project(7..8);
        assert!(result.is_err(), "range exceeds registration");
        let Err(rejected) = result else { return };
        let (held, error) = rejected.into_parts();
        assert_eq!((error.start(), error.end(), error.len()), (7, 8, 7));
        assert_eq!(held.as_slice(), b"exactly");
        assert_eq!(held.as_slice().as_ptr(), pointer);
        assert_eq!(budget.resident(), 7);
        assert!(!arena.has_returned());
        let empty = held.project(7..7).expect("empty endpoint");
        assert!(empty.as_slice().is_empty());
        assert_eq!(budget.resident(), 0);
        assert!(arena.has_returned());
    })
    .expect("provided charge fixture");
}

#[test]
fn bounds_nested_prefix() {
    use buffer::{
        bytes::bounded::{self, raw::Input},
        raw::Freeze as _,
    };

    Item::try_scope(|unique, budget, arena| {
        let held = {
            let proof = unsafe { Input::new(unique) };
            let mut source = bounded::Value::from_raw(proof);
            let pointer = source.as_ref().as_ptr();
            let four = NonZero::<usize>::MIN.saturating_add(3);
            let mut prefix = source.try_take(four).expect("four-byte prefix");
            let head = prefix
                .try_take(NonZero::<usize>::MIN.saturating_add(2))
                .expect("three-byte nested prefix");
            let two = NonZero::<usize>::MIN.saturating_add(1);
            let result = prefix.try_take(two);
            assert!(result.is_err(), "one byte remains");
            let Err(error) = result else { return };
            assert_eq!(error.into_parts(), (two, 1));
            assert_eq!(prefix.as_ref(), b"c");
            let head = head.freeze();
            let held = head.try_account(&budget).expect("original registration");
            source.as_mut()[0] = b'T';
            assert_eq!(source.as_ref(), b"Tly");
            assert_eq!(held.as_ref(), b"exa");
            assert_eq!(held.as_ref().as_ptr(), pointer);
            held
        };
        assert_eq!(budget.resident(), 7);
        assert!(!arena.has_returned());
        assert_eq!(held.as_ref(), b"exa");
    })
    .expect("provided charge fixture");
}

#[test]
fn binds_vector() {
    let mut bytes = b"payload".to_vec();
    let sibling = {
        let result = storage::Shared::try_from(&mut bytes);
        let source = result.expect("vector control block allocates");
        let (result, activity) = Tracker::measure(|| source.try_clone());
        assert_eq!(activity, [0; 4]);
        let sibling = result.expect("vector edge retains");
        let sibling = resident::Shared::from(sibling);
        let views = source.backing().zip(sibling.backing());
        let (left, right) = views.expect("both vector owners have backing");
        assert!(left.contains(&right) && right.contains(&left));
        assert_eq!(left.resident(), right.resident());
        sibling
    };
    let (result, activity) = Tracker::measure(|| sibling.try_detach());
    assert_eq!(activity, [0; 4]);
    let sibling = result
        .map_err(|owner| owner.as_slice().len())
        .expect("sole buffer owner must detach");
    assert_eq!(sibling.as_slice(), b"payload");
}

#[test]
fn clones_allocation() {
    Capacity(14)
        .try_scope(|budget| {
            {
                let sibling = {
                    let result = resident::Exact::try_copy(&budget, b"payload");
                    let source = result.expect("first allocation fits budget");
                    let source = source.into_shared();
                    let (result, activity) = Tracker::measure(|| source.try_clone());
                    assert_eq!(activity, [0; 4]);
                    let sibling = result.expect("allocation edge retains");
                    let result = resident::Exact::try_copy(&budget, b"payload");
                    let foreign = result.expect("second allocation fits budget");
                    let foreign = foreign.into_shared();
                    let views = source
                        .backing()
                        .zip(sibling.backing())
                        .zip(foreign.backing());
                    let ((left, right), other) = views.expect("all allocation owners have backing");
                    assert!(left.contains(&right) && right.contains(&left));
                    assert!(!left.contains(&other) && !other.contains(&left));
                    assert_eq!(left.resident(), 7);
                    sibling
                };
                assert_eq!(budget.resident(), 7);
                assert_eq!(sibling.as_slice(), b"payload");
            }
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}
