pub mod common;

use common::buffer::Redirect;
use o3::buffer::{
    self, Complete as _, Residency as _, pool, resident, segments::Drain, storage,
    write::consume::Consume,
};

#[test]
fn rejects_oversize() {
    let mut bytes = storage::Shared::try_from(&mut b"abc".to_vec())
        .expect("vector sharing must allocate metadata");
    let error = bytes.try_consume(4).expect_err("three-byte target");
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (4, 3));
    assert_eq!(bytes.as_slice(), b"abc");
    bytes
        .try_consume(1)
        .expect("a valid prefix remains consumable after rejection");
    assert_eq!(bytes.as_slice(), b"bc");
    assert_eq!(bytes.consume_max(1), 1);
    bytes.try_consume(1).expect("the remaining one-byte target");
    assert!(bytes.as_slice().is_empty());
}

#[test]
fn rejects_redirected_bound() {
    let created = storage::Shared::try_copy(b"x");
    let target = created.expect("required fixture outcome");
    let mut routed = Redirect {
        source: *b"abc",
        target,
    };
    assert_eq!(routed.remaining(), 3);
    assert_eq!(
        routed.try_consume(2).map_err(|error| {
            let (required, available) = error.into_parts();
            (required.get(), available)
        }),
        Err((2, 1))
    );
    assert_eq!(routed.target.as_slice(), b"x");
    assert_eq!(routed.consume_max(2), 1);
    assert!(routed.target.is_empty());
    assert_eq!(routed.source.as_slice(), b"abc");
}

#[test]
fn cancels_drain() {
    use o3::buffer::segments::queue;

    let created = storage::Shared::try_copy(b"abcdef");
    let bytes = created.expect("required fixture outcome");
    use o3::depot::capacity::Count;

    let created = queue::Fifo::try_with(queue::Capacity::new(Count::new(6), Count::new(1)));
    let mut queue = created.expect("required fixture outcome");
    let pushed = queue.try_push(bytes);
    pushed.as_ref().expect("required fixture outcome");
    {
        let prepared = Drain::try_new(&mut queue, 3);
        let drain = prepared.expect("required fixture outcome");
        assert_eq!(
            std::mem::size_of_val(&drain),
            2 * std::mem::size_of::<usize>()
        );
    }
    assert_eq!(queue.len(), 6);
    let prepared = Drain::try_new(&mut queue, 3);
    let drain = prepared.expect("required fixture outcome");
    drain.commit();
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.cursor().contiguous(0..3), Some(b"def".as_slice()));
}

#[test]
fn keeps_source_borrow() {
    let owner = resident::Shared::from(b"abcdef".as_slice());
    let narrowed = {
        let view = owner.get(1..5);
        let view = view.expect("assertion failed: view.is_some()");
        view.get(1..3)
    };
    let narrowed = narrowed.expect("assertion failed: narrowed.is_some()");
    assert_eq!(narrowed.as_slice(), b"cd");
    assert_eq!(narrowed.as_slice().as_ptr(), owner.as_slice()[2..].as_ptr());
}

#[test]
fn projects_vec_range() {
    let mut bytes = Vec::from(&b"0123456789"[..]);
    let base = bytes.as_ptr();
    let capacity = bytes.capacity();
    let owner =
        (storage::Shared::try_from(&mut bytes)).expect("vector sharing must allocate metadata");
    let shared = owner.project(2..8).expect("the vector range must project");

    assert_eq!(shared.as_slice().as_ptr(), base.wrapping_add(2));
    assert_eq!(shared.as_slice(), b"234567");
    assert_eq!(shared.resident(), capacity);

    let view = shared
        .try_get(1..5)
        .expect("the shared range retains")
        .expect("the range is contained");
    assert_eq!(view.as_slice().as_ptr(), base.wrapping_add(3));
    assert_eq!(view.as_slice(), b"3456");
    assert_eq!(view.resident(), capacity);
}

#[test]
fn keeps_backing_layout() {
    let shared =
        (storage::Shared::try_copy(b"0123456789")).expect("the backing-layout owner must allocate");
    let part = shared
        .try_get(2..8)
        .expect("the shared range retains")
        .expect("the range is contained");
    let root = (shared.backing()).expect("the root owner is nonempty");
    let view = (part.backing()).expect("the projected owner is nonempty");

    assert!(root.contains(&view));
    assert!(!view.contains(&root));
    assert_eq!(view.len(), 6);
    assert_eq!(view.resident(), root.resident());
}

#[test]
fn copies_distinct_backing() {
    let source =
        (storage::Shared::try_copy(b"materialized")).expect("the source backing must allocate");
    let copied = (storage::Shared::try_copy(source.as_slice()))
        .expect("the copied backing must allocate independently");

    assert!(
        source
            .backing()
            .zip(copied.backing())
            .is_some_and(|(source, copied)| !source.contains(&copied))
    );
}

#[test]
fn bounds_follow_window() {
    let owner = storage::Shared::try_copy(b"abcdefgh");
    let owner = owner.expect("required fixture outcome");
    let window = owner.view().try_get(2..6);
    let window = window.expect("required fixture outcome");
    for start in 0..=6 {
        for end in 0..=6 {
            let result = window.try_get(start..end);
            if start <= end && end <= 4 {
                let view = result.expect("required fixture outcome");
                let retained = view.try_retain();
                let retained = retained.expect("required fixture outcome");
                assert_eq!(retained.as_slice(), &b"cdef"[start..end]);
                if !retained.is_empty() {
                    assert_eq!(retained.as_ptr(), view.as_slice().as_ptr());
                }
            } else {
                let error = result.err().expect("invalid nested bounds");
                assert_eq!((error.start(), error.end(), error.len()), (start, end, 4));
            }
        }
    }
}

#[test]
fn releases_empty_ranges() {
    let pool = (pool::Pool::try_new(1, 16)).expect("one byte slot must allocate");
    let frozen = pool
        .try_copy(b"abc")
        .expect("three bytes fit the slot")
        .expect("the slot is vacant");
    let shared = frozen.into_shared();
    let first = shared
        .try_get(1..1)
        .expect("empty range needs no retained edge")
        .expect("empty range is contained");
    let second = buffer::raw::Segment::try_get(&shared, 2..2)
        .expect("empty range needs no retained edge")
        .expect("empty range is contained");
    let third = buffer::raw::Segment::prefix(shared, 0);
    assert_eq!(pool.available(), 1);
    assert!(first.is_empty());
    assert!(second.as_slice().is_empty());
    assert!(third.as_slice().is_empty());
}
