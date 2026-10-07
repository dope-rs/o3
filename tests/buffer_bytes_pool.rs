pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::num::NonZero;

use common::{alloc::system::Tracker, buffer::domain::Capacity};
use o3::buffer::{
    self,
    bytes::{TryRetain, scope},
    pool, resident, storage,
    write::consume::{self, Consume as _},
};

#[test]
fn promotes_scoped_pool() {
    let held = {
        let created = pool::Pool::try_new(1, 16);
        let pool = created.expect("scoped retention pool must be constructible");
        let mut cursor = pool.view().expect("pool slot");
        {
            let mut spare = cursor.spare();
            assert_eq!(spare.write(b"abcdef"), b"abcdef".len());
            spare.commit();
        }
        let source = cursor.as_ref()[1..5].as_ptr();
        let result = cursor.freeze().project(1..5);
        let pooled = result.expect("the pooled range must project");

        assert_eq!(pooled.as_slice(), b"bcde");
        assert_eq!(pooled.as_slice().as_ptr(), source);
        assert_eq!(pool.available(), 0);

        let retained = TryRetain::try_retain(pooled);
        let held = retained.expect("pooled bytes must retain in place");
        assert_eq!(held.as_slice().as_ptr(), source);
        held
    };

    assert_eq!(held.as_slice(), b"bcde");
}

#[test]
fn normalizes_frozen_range() {
    let created = pool::Pool::try_new(1, 16);
    let pool = created.expect("frozen-range pool must be constructible");
    let mut lease = pool.acquire().expect("pool slot");
    lease
        .try_append(b"abcdef")
        .expect("required fixture outcome")
        .commit();
    let frozen = lease.freeze().into_shared();
    {
        let peer = frozen.try_clone().expect("asserted retained edge");
        let source = frozen.as_ref()[1..5].as_ptr();
        {
            let result = (frozen).project(1..5);
            let held = result.expect("the frozen range must project");
            let shared = held.try_clone().expect("asserted retained edge");
            assert_eq!(shared.as_slice(), b"bcde");
            assert_eq!(shared.as_slice().as_ptr(), source);
            assert!(shared.backing().zip(held.backing()).is_some_and(|(left, right)| left.contains(&right) && left.len() == right.len()));
        }
        assert_eq!(pool.available(), 0, "independent frozen sibling remains");
        assert_eq!(peer.as_ref(), b"abcdef");
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn shares_frozen_domain() {
    let created = pool::Pool::try_new(1, 16);
    let pool = created.expect("frozen-domain pool must be constructible");
    let mut lease = pool.acquire().expect("pool slot");
    lease
        .try_append(b"abcdef")
        .expect("required fixture outcome")
        .commit();
    let first = lease.freeze().into_shared();
    let second = first.try_clone().expect("asserted retained edge");
    let source = first.as_ref().as_ptr();

    {
        let second = {
            let first = first;
            let second = second;
            assert_eq!(first.as_slice().as_ptr(), source);
            assert_eq!(second.as_slice().as_ptr(), source);
            assert!(first.backing().zip(second.backing()).is_some_and(|(left, right)| left.contains(&right) && left.len() == right.len()));
            assert_eq!(pool.available(), 0);
            second
        };
        assert_eq!(pool.available(), 0);
        assert_eq!(second.as_slice().as_ptr(), source);
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn releases_empty_pool() {
    let created = pool::Pool::try_new(1, 8);
    let pool = created.expect("empty-owned pool must be constructible");
    let mut lease = pool.acquire().expect("pool slot");
    lease
        .try_append(b"abcdef")
        .expect("required fixture outcome")
        .commit();

    assert_eq!(pool.available(), 0);
    let result = lease.freeze().into_shared().project(3..3);
    let bytes = result.expect("the empty owned range must project");
    assert!(bytes.is_empty());
    assert_eq!(pool.available(), 1);
}

#[test]
fn releases_empty_borrow() {
    let created = pool::Pool::try_new(1, 8);
    let pool = created.expect("empty-borrow pool must be constructible");
    let mut cursor = pool.view().expect("pool slot");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"abcdef"), b"abcdef".len());
        spare.commit();
    }

    assert_eq!(pool.available(), 0);
    let result = cursor.freeze().project(3..3);
    let bytes = result.expect("the empty borrowed range must project");
    assert!(bytes.is_empty());
    assert_eq!(pool.available(), 1);
}

#[test]
fn slices_without_copying() {
    let created = pool::Pool::try_new(1, 8);
    let pool = created.expect("slicing pool must be constructible");
    {
        let mut lease = pool.acquire().expect("pool slot");
        lease
            .try_append(b"abcdef")
            .expect("required fixture outcome")
            .commit();
        let source = lease.as_slice().as_ptr();
        let result = lease.freeze().into_shared().project(1..5);
        let mut held = result.expect("the retained pool range must project");
        assert_eq!(held.as_slice(), b"bcde");
        assert_eq!(held.as_slice().as_ptr(), source.wrapping_add(1));
        assert_eq!(pool.available(), 0);
        consume::Consume::try_consume(&mut held, 1)
            .as_ref()
            .expect("required fixture outcome");
        assert_eq!(held.as_slice(), b"cde");
        assert_eq!(held.as_slice().as_ptr(), source.wrapping_add(2));
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn pooled_retains_in_place() {
    let created = pool::Pool::try_new(1, 16);
    let pool = created.expect("in-place retention pool must be constructible");
    let mut cursor = (pool.view()).expect("single pool slot is available");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"pooled"), b"pooled".len());
        spare.commit();
    }
    let pooled = cursor.freeze();
    let pointer = pooled.as_slice().as_ptr();

    let allocs = {
        let (result, allocs) = Tracker::count(|| pooled.try_retain());
        let retained = result.expect("pooled bytes must retain in place");
        assert_eq!(retained.as_slice().as_ptr(), pointer);
        assert_eq!(pool.available(), 0);
        allocs
    };
    assert_eq!(allocs, 0);
    assert_eq!(pool.available(), 1);
}

#[test]
fn retains_live_charge() {
    for bytes in [b"bodyTRAILER".as_slice(), b"TRAILER".as_slice()] {
        Capacity(64)
            .try_scope(|budget| {
                {
                    let source = resident::Exact::try_copy(&budget, bytes)
                        .expect("source allocation succeeds")
                        .into_shared();
                    let pointer = source.as_ptr();
                    let charge = budget.resident();
                    let (result, count) = Tracker::count(|| {
                        scope::Owner::with(source, |owner| {
                            let (tail, range) = owner.view().try_rsplit::<7>()?;
                            Ok::<_, consume::Error>((tail[0], owner.project(range)))
                        })
                    });
                    assert_eq!(count, 0);
                    let (header, payload) = result.expect("trailer fits");
                    assert_eq!(header, b'T');
                    assert_eq!(payload.as_slice(), &bytes[..bytes.len() - 7]);
                    assert_eq!(
                        budget.resident(),
                        if payload.is_empty() { 0 } else { charge }
                    );
                    if !payload.is_empty() {
                        assert_eq!(payload.as_ptr(), pointer);
                    }
                }
                assert_eq!(budget.resident(), 0);
            })
            .expect("resident test domain must admit its budget");
    }
}

#[test]
fn failure_releases_charge() {
    Capacity(64)
        .try_scope(|budget| {
            let source = (resident::Exact::try_copy(&budget, b"bodyTRAILER"))
                .expect("source allocation succeeds");
            let source = source.into_shared();
            let (result, count) = Tracker::count(|| {
                scope::Owner::with(source, |owner| {
                    let (tail, range) = owner.view().try_rsplit::<12>()?;
                    let header = tail[0];
                    Ok::<_, consume::Error>((header, owner.project(range)))
                })
            });
            assert_eq!(count, 0);

            let error = result.expect_err("trailer exceeds the source");
            assert_eq!(
                error.into_parts(),
                (NonZero::<usize>::MIN.saturating_add(11), 11)
            );
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn clamps_owned_prefix() {
    for max in [0, 1, 3, 4, usize::MAX] {
        let source = storage::Shared::try_copy(b"body").expect("source allocation");
        let clone = source.try_clone().expect("shared edge");
        let (selected, hot) = Tracker::measure(|| buffer::raw::Segment::prefix(clone, max));
        assert_eq!(hot, [0; 4]);
        assert_eq!(selected.as_slice(), &b"body"[..max.min(4)]);
        if max != 0 {
            assert_eq!(selected.as_ptr(), source.as_ptr());
        }
    }
}

#[test]
fn clamps_resident_prefix() {
    for max in [0, 1, 3, 4, usize::MAX] {
        Capacity(4)
            .try_scope(|budget| {
                {
                    let source = resident::Exact::try_copy(&budget, b"body")
                        .expect("resident allocation")
                        .into_shared();
                    let pointer = source.as_ptr();
                    let (selected, count) =
                        Tracker::count(|| buffer::raw::Segment::prefix(source, max));
                    assert_eq!(count, 0);
                    assert_eq!(selected.as_slice(), &b"body"[..max.min(4)]);
                    assert_eq!(budget.resident(), if max == 0 { 0 } else { 4 });
                    if max != 0 {
                        assert_eq!(selected.as_ptr(), pointer);
                    }
                }
                assert_eq!(budget.resident(), 0);
            })
            .expect("resident test domain must admit its budget");
    }
}

#[test]
fn keeps_empty_contracts() {
    for mut source in [
        resident::Shared::new(),
        resident::Shared::from(b"".as_slice()),
        resident::Shared::from(storage::Shared::default()),
    ] {
        let error = source.try_consume(1).expect_err("empty source");
        assert_eq!(error.into_parts(), (NonZero::<usize>::MIN, 0));
        source.try_consume(0).expect("empty prefix fits");
        for max in [0, 1, usize::MAX] {
            let clone = source.try_clone().expect("empty clone");
            let (prefix, hot) = Tracker::measure(|| buffer::raw::Segment::prefix(clone, max));
            assert_eq!(hot, [0; 4]);
            assert!(prefix.is_empty());
            assert!(matches!(prefix.scope(), resident::Scope::Independent));
            assert!(matches!(prefix.try_detach(), Ok(owner) if owner.is_empty()));
        }
        scope::Owner::with(source, |owner| {
            let error = owner.view().try_rsplit::<1>().err().expect("empty source");
            assert_eq!(error.into_parts(), (NonZero::<usize>::MIN, 0));
            let (_, range) = owner
                .view()
                .try_rsplit::<0>()
                .expect("empty selection fits");
            assert!(owner.project(range).is_empty());
        });
    }
}

#[test]
fn consumes_resident_range() {
    Capacity(4)
        .try_scope(|budget| {
            let mut source = resident::Exact::try_copy(&budget, b"body")
                .expect("resident allocation")
                .into_shared();
            let pointer = source.as_ptr();
            for amount in [5, usize::MAX] {
                let (result, hot) = Tracker::measure(|| source.try_consume(amount));

                let error = result.expect_err("prefix exceeds the source");
                assert_eq!(
                    error.into_parts(),
                    (NonZero::new(amount).expect("positive request"), 4)
                );
                assert_eq!(hot, [0; 4]);
                assert_eq!(source.as_slice(), b"body");
                assert_eq!(source.as_ptr(), pointer);
                assert_eq!(budget.resident(), 4);
            }
            let (result, hot) = Tracker::measure(|| source.try_consume(2));
            result.expect("two byte prefix fits");
            assert_eq!(hot, [0; 4]);
            assert_eq!(source.as_slice(), b"dy");
            assert_eq!(source.as_ptr(), pointer.wrapping_add(2));
            assert_eq!(budget.resident(), 4);
            assert_eq!(source.consume_max(usize::MAX), 2);
            assert!(source.is_empty());
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}
