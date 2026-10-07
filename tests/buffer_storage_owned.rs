pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, mem, num, pin};

use common::{
    Counted,
    alloc::system::Tracker,
    buffer::{
        domain::Capacity,
        provided::scope::{Context, Error, Phase, Run, Terminal},
    },
};
use o3::{
    alloc,
    base::cell::region,
    buffer::{
        self, BLOCK_CAPACITY, Residency as _,
        bytes::bounded,
        raw::Arenas as _,
        resident,
        storage::{self, Owned, provided},
        write::{Sink as _, SpareExt as _, consume},
    },
};

type FixedOwned = Owned<(), BLOCK_CAPACITY>;
const FIXED_CAPACITY: usize = BLOCK_CAPACITY as usize;

#[test]
fn preserves_nonempty_owner() {
    use storage::NonEmpty;

    let bytes = Vec::with_capacity(4);
    let pointer = bytes.as_ptr();
    let (bytes, cause) = NonEmpty::try_new(bytes)
        .expect_err("empty owner")
        .into_parts();
    assert_eq!(cause, storage::Zero);
    assert_eq!((bytes.as_ptr(), bytes.capacity()), (pointer, 4));
    let (bytes, cause) = NonEmpty::try_resize(bytes, num::NonZero::<usize>::MAX, 0)
        .expect_err("unrepresentable byte extent")
        .into_parts();
    assert_eq!(cause, alloc::Error::Overflow);
    let bytes = NonEmpty::try_push(bytes, 1)
        .expect("reserved byte")
        .into_storage();
    let mut bytes = NonEmpty::try_resize(bytes, num::NonZero::<usize>::MIN.saturating_add(1), 2)
        .expect("reserved extent");
    bytes.append().write_byte(3).expect("reserved suffix");
    *bytes.first_mut() = 4;
    bytes[1] = 5;
    bytes.truncate(num::NonZero::<usize>::MIN);
    assert_eq!(bytes.len(), num::NonZero::<usize>::MIN);
    let bytes = bounded::Value::try_new(bytes)
        .map_err(|error| error.into_parts().1)
        .expect("bounded nonempty owner")
        .into_storage::<Vec<u8>>()
        .into_inner();
    assert_eq!(
        (bytes.as_ptr(), bytes.capacity(), bytes.as_slice()),
        (pointer, 4, &[4][..])
    );
}

#[test]
fn copies_and_freezes() {
    let mut owned = (FixedOwned::try_new()).expect("the fixed test buffer must allocate");
    owned
        .try_extend(b"fixed block")
        .expect("required fixture outcome");

    let ptr = owned.as_ptr();
    let shared = owned.freeze();
    assert_eq!(shared.as_ptr(), ptr);
    assert_eq!(shared.as_slice(), b"fixed block");
    assert_eq!(shared.resident(), FIXED_CAPACITY);
    assert_eq!(
        shared
            .try_clone()
            .expect("asserted retained edge")
            .as_slice(),
        b"fixed block"
    );
}

#[test]
fn shares_large_vec() {
    let mut data = Vec::with_capacity(8192);
    data.resize(4096, b'x');
    let ptr = data.as_ptr();
    let cap = data.capacity();
    let slice = {
        let shared = (storage::Shared::try_from(&mut data))
            .expect("freezing the initialized vector must succeed");
        assert_eq!(shared.as_ptr(), ptr);
        assert_eq!(shared.resident(), cap);
        shared
            .try_get(1024..3072)
            .expect("asserted retained edge")
            .expect("the requested range lies within the shared Vec")
    };
    assert_eq!(slice.as_slice(), &[b'x'; 2048]);
    assert_eq!(slice.as_ptr(), ptr.wrapping_add(1024));
    assert_eq!(slice.resident(), cap);
}

#[test]
fn commits_spare() {
    let mut owned = (FixedOwned::try_new()).expect("the fixed test buffer must allocate");
    let mut spare = owned.spare();
    spare.write_slice(b"xyz").expect("required fixture outcome");
    spare.commit();
    assert_eq!(owned.as_slice(), b"xyz");

    let mut writer = owned.spare();
    writer
        .write_slice(b"raw")
        .expect("required fixture outcome");
    assert_eq!(writer.len(), 3);
    writer.commit();
    assert_eq!(owned.as_slice(), b"xyzraw");
}

#[test]
fn fills_fixed_capacity() {
    let bytes = vec![b'x'; FIXED_CAPACITY];
    let mut owned = (FixedOwned::try_new()).expect("the fixed test buffer must allocate");
    owned.try_extend(&bytes).expect("required fixture outcome");
    assert_eq!(owned.len(), FIXED_CAPACITY);

    let error = owned.try_extend(b"y").expect_err("full fixed buffer");
    assert_eq!(
        {
            let (required, available) = error.into_parts();
            (required.get(), available)
        },
        (1, 0)
    );
    assert_eq!(owned.as_slice(), bytes);
}

#[test]
fn rejects_oversized_write() {
    let mut owned = (FixedOwned::try_new()).expect("the fixed test buffer must allocate");
    owned
        .try_extend(b"prefix")
        .expect("required fixture outcome");
    let excess = vec![0; FIXED_CAPACITY];

    let error = owned.try_extend(&excess).expect_err("oversized write");
    assert_eq!(
        {
            let (required, available) = error.into_parts();
            (required.get(), available)
        },
        (FIXED_CAPACITY, FIXED_CAPACITY - b"prefix".len())
    );
    assert_eq!(owned.as_slice(), b"prefix");
}

#[test]
fn keeps_runtime_capacity() {
    let mut owned = (Owned::try_with(5)).expect("the five-byte test buffer must allocate");
    owned
        .try_extend(b"exact")
        .expect("required fixture outcome");
    assert_eq!(owned.as_slice(), b"exact");
    assert!(owned.try_extend(b"!").is_err());
}

#[test]
fn fills_exact_allocation() {
    let owned = (Owned::try_filled(4, b'x')).expect("the four-byte filled buffer must allocate");
    assert_eq!(owned.as_slice(), b"xxxx");
}

#[test]
fn commits_built_prefix() {
    let result = Owned::try_build(16, |out| {
        out.write_slice(b"short")?;
        Ok::<_, consume::Error>(())
    });
    let owned = (result).expect("the prefix writer must commit its bytes");

    assert_eq!(owned.as_slice(), b"short");
}

#[test]
fn empty_owners_skip_alloc() {
    let (built, allocs) = Tracker::count(|| -> Result<_, storage::AllocError> {
        let empty = storage::Owned::new();
        let explicit = storage::Owned::try_with(0)?;
        let filled = storage::Owned::try_filled(0, 7)?;
        Ok((empty, explicit, filled))
    });
    assert_eq!(allocs, 0);
    let (empty, explicit, filled) =
        (built).expect("all empty storage owners must initialize without allocation");
    for mut owner in [empty, explicit, filled] {
        assert_eq!(owner.spare().remaining(), 0);
        assert!(owner.is_empty());
    }

    Capacity(8)
        .try_scope(|budget| {
            let (created, allocs) = Tracker::count(|| storage::Owned::try_allocate(&budget, 0));
            assert_eq!(allocs, 0);
            let owner =
                (created).expect("the empty resident owner must initialize without allocation");
            assert_eq!(owner.capacity(), 0);
            assert!(owner.is_empty());
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn freezes_in_place() {
    let (created, allocs) = Tracker::count(|| storage::Owned::try_with(8));
    assert_eq!(allocs, 1);
    let mut local = (created).expect("the measured eight-byte storage owner must allocate once");
    let ptr = local.as_ptr();
    let (shared, allocs) = Tracker::count(|| {
        local.try_extend(b"payload").expect("the payload must fit");
        local.freeze()
    });
    assert_eq!(allocs, 0);
    assert_eq!(shared.as_ptr(), ptr);

    Capacity(8)
        .try_scope(|budget| {
            {
                let (created, allocs) = Tracker::count(|| storage::Owned::try_allocate(&budget, 8));
                assert_eq!(allocs, 1);
                let mut owner = (created).expect("the measured resident owner must allocate once");
                let ptr = owner.as_ptr();
                let (frozen, allocs) = Tracker::count(|| {
                    let mut spare = owner.spare();
                    spare
                        .write_slice(b"resident")
                        .expect("the payload must fit");
                    spare.commit();
                    owner
                        .try_compact()
                        .expect("the full owner must compact in place");
                    assert_eq!(owner.capacity(), 8);
                    owner.try_freeze()
                });
                assert_eq!(allocs, 0);
                let shared = frozen
                    .map_err(|owner| owner.len())
                    .expect("full resident owner must freeze");
                assert_eq!(shared.as_slice().as_ptr(), ptr);
                assert_eq!(budget.resident(), 8);
                assert_eq!(shared.as_slice(), b"resident");
            }
            assert_eq!(budget.resident(), 0);

            let mut owner = (storage::Owned::try_allocate(&budget, 8))
                .expect("the compact-failure resident owner must allocate");
            let ptr = owner.as_ptr();
            let mut spare = owner.spare();
            assert_eq!(spare.write(b"part"), 4);
            spare.commit();
            let compact = Tracker::fail_after(0, || owner.try_compact());
            assert!(compact.is_err());
            assert_eq!(owner.as_ptr(), ptr);
            assert_eq!(&owner[..], b"part");
            assert_eq!(owner.capacity(), 8);
            assert_eq!(budget.resident(), 8);
            {
                let (shared, allocs) = Tracker::count(|| owner.freeze());
                assert_eq!(allocs, 0);
                assert_eq!(shared.as_ptr(), ptr);
                assert_eq!(shared.as_slice(), b"part");
                assert_eq!(shared.resident(), 8);
                assert_eq!(budget.resident(), 8);
            }
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn fixed_scope() {
    let drops = Cell::new(Some(0));
    let mut domain = resident::Domain::new();
    let mut arena =
        provided::Arena::<_, _, ()>::try_allocate(num::NonZero::<u32>::MIN.saturating_add(2))
            .expect("three fixed slots must allocate")(Context);
    let ((), activity) = Tracker::activity(|| {
        arena.scope(pin::Pin::new(&mut domain), |token, root, domain| {
            let budget = (domain.get_mut().try_budget(32, token.proof()))
                .expect("fresh budget must activate");
            let other = (budget.try_charge(7)).expect("test precondition");
            let vacant = (root.try_vacant()).expect("test precondition");
            let item = (*b"data", Counted(&drops));
            // SAFETY: Context's fixed layout is exactly this item's initialized four bytes.
            let source = unsafe { buffer::raw::Fixed::insert(vacant, item) };
            let shared = source.into_shared();
            let first = (shared.try_account(&budget)).expect("test precondition");
            let second = (first.try_clone()).expect("test precondition");
            assert_eq!(budget.resident(), 11);
            let first = resident::Shared::from(first);
            let second = resident::Shared::from(second);
            let view = (first.backing()).expect("test precondition");
            let sibling = (second.backing()).expect("test precondition");
            assert_eq!(view.resident(), 4);
            assert!(view.contains(&sibling));
            mem::forget(first);
            mem::forget(second);
            mem::forget(shared);
            mem::forget(other);
            let vacant = (root.try_vacant()).expect("test precondition");
            let item = (*b"more", Counted(&drops));
            // SAFETY: the same context retains this complete four-byte item's fixed layout.
            mem::forget(unsafe { buffer::raw::Fixed::insert(vacant, item) });
            let vacant = (root.try_vacant()).expect("test precondition");
            mem::forget(vacant);
        })
    });
    assert_eq!((activity.allocations, activity.deallocations), (0, 0));
    assert_eq!(drops.get(), Some(2));
    region::Token::scope_pinned(pin::Pin::new(&mut domain), |token, domain| {
        assert!(
            matches!(domain.get_mut().try_budget(32, token.proof()), Err(error) if error.retained.get() == 7)
        );
    });
    let (context, activity) = Tracker::activity(|| arena.into_context());
    assert_eq!((activity.allocations, activity.deallocations), (0, 1));
    let Context = context;
}

#[test]
fn exact_scope() {
    let drops = Cell::new(Some(0));
    let mut domain = resident::Domain::new();
    let mut arena =
        (provided::Arena::<_, _>::try_with(num::NonZero::<u32>::MIN, || Ok::<_, alloc::Error>(())))
            .expect("test precondition");
    for _ in 0..2 {
        let measured = Tracker::begin();
        arena.scope(pin::Pin::new(&mut domain), |token, root, domain| {
            let budget =
                (domain.get_mut().try_budget(4, token.proof())).expect("test precondition");
            {
                let vacant = (root.vacant()).expect("test precondition");
                let source = (vacant
                    .try_insert((*b"data", Counted(&drops)), 4, |item| &mut item.0))
                .expect("buffer transition accepted");
                let held = (source.try_account(&budget)).expect("buffer transition accepted");
                assert_eq!(held.as_slice(), b"data");
                assert_eq!(budget.resident(), 4);
                assert!(!root.has_returned());
            }
            assert_eq!(budget.resident(), 0);
            let item = root.pop_returned().expect("last account returns its item");
            let vacant = root.vacant().expect("returned slot must be vacant");
            let source = vacant
                .try_insert(item, 4, |item| &mut item.0)
                .expect("buffer transition accepted");
            let held = source
                .try_account(&budget)
                .expect("buffer transition accepted");
            mem::forget(held);
        });
        let activity = measured.finish();
        assert_eq!((activity.allocations, activity.deallocations), (0, 0));
    }
    assert_eq!(drops.get(), Some(2));
    region::Token::scope_pinned(pin::Pin::new(&mut domain), |token, domain| {
        domain
            .get_mut()
            .try_budget(4, token.proof())
            .expect("required fixture outcome");
    });
}

#[test]
fn terminal_scope() {
    for limit in [0, 1, 2] {
        let drops = Cell::new(Some(0));
        let mut domain = resident::Domain::new();
        let mut arena =
            provided::Arena::<_, _, ()>::try_allocate(num::NonZero::<u32>::MIN.saturating_add(1))
                .expect("test precondition")(Context);
        {
            let mut terminal = Terminal {
                phase: Phase::Active,
                items: [None, None],
                returned: 0,
                error: None,
            };
            let (result, activity) = Tracker::activity(|| {
                arena.run(
                    pin::Pin::new(&mut domain),
                    limit,
                    &mut terminal,
                    Run { drops: &drops },
                )
            });
            assert_eq!((activity.allocations, activity.deallocations), (0, 0));
            match limit {
                0 | 1 => assert!(matches!(
                    result,
                    Err(Error::Account(provided::account::Cause::Exhausted { count, limit: actual }))
                        if count == limit && actual == limit
                )),
                _ => assert_eq!(result.expect("all fixed slots can be accounted"), (8, 0)),
            }
            assert!(terminal.error.is_none(), "{:?}", terminal.error);
            assert_eq!(terminal.returned, (limit + 1).min(2) as usize);
            assert_eq!(drops.get(), Some(0));
            region::Token::scope_pinned(pin::Pin::new(&mut domain), |token, domain| {
                domain
                    .get_mut()
                    .try_budget(8, token.proof())
                    .expect("required fixture outcome");
            });
        }
        assert_eq!(drops.get(), Some((limit + 1).min(2) as usize));
    }
}

#[test]
fn joined_scopes() {
    let drops = Cell::new(Some(0));
    let first =
        provided::Arena::<_, _>::try_with(num::NonZero::<u32>::MIN, || Ok::<_, alloc::Error>(()))
            .expect("first arena");
    let second =
        provided::Arena::<_, _>::try_with(num::NonZero::<u32>::MIN, || Ok::<_, alloc::Error>(()))
            .expect("second arena");
    let mut state = (resident::Domain::new(), (first, second));
    for round in 1..=2 {
        region::Token::scope_pinned(pin::Pin::new(&mut state), |token, state| {
            let (domain, arenas) = state.get_mut();
            let budget = domain
                .try_budget(8, token.proof())
                .expect("new accounting generation");
            // SAFETY: all issued roots stay in this generation; the terminal runs
            // before its original domain ends, with no remaining accessible owner.
            let ((first, second), finish) = unsafe { arenas.enter() };
            for root in [first, second] {
                let vacant = root.vacant().expect("one slot per arena");
                let owner = vacant
                    .try_insert((*b"held", Counted(&drops)), 4, |item| &mut item.0)
                    .expect("buffer transition accepted");
                let held = owner
                    .try_account(&budget)
                    .expect("buffer transition accepted");
                let alias = held.try_clone().expect("proved clone");
                mem::forget(held);
                mem::forget(alias);
            }
            assert_eq!(budget.resident(), 8);
            finish();
            assert_eq!(budget.resident(), 0);
            assert_eq!(drops.get(), Some(round * 2));
        });
    }
}
