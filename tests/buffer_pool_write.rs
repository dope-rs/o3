pub mod common;

use common::alloc::system::{Activity, Tracker};
use o3::buffer::{
    self, bytes, pool,
    write::{self, Sink, SpareExt as _, consume::Consume},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn freeze_keeps_tail() {
    let created = pool::Pool::try_new(1, 16);
    let pool = created.expect("freeze pool must contain one sixteen-byte slot");
    let mut buffer = (pool.cursor()).expect("one slot is available");
    Sink::write_slice(&mut buffer, b"discardpayload")
        .as_ref()
        .expect("required fixture outcome");
    let ptr = buffer.as_ref()[7..].as_ptr();
    let consumed = buffer.try_consume(7);
    consumed
        .as_ref()
        .expect("initialized discard prefix must be consumable");

    {
        let frozen = buffer.freeze();
        assert_eq!(frozen.as_slice(), b"payload");
        assert_eq!(frozen.as_slice().as_ptr(), ptr);
        assert_eq!(pool.available(), 0);
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn exact_requires_full() {
    let created = pool::Pool::try_new(1, 16);
    let pool: pool::Pool = created.expect("exact-write pool must contain one sixteen-byte slot");
    let mut buffer = (pool.acquire()).expect("one slot is available");
    let mut writer = buffer.spare();
    {
        let planned = writer.try_exact(4);
        let mut exact = planned.expect("four-byte exact plan must fit empty spare storage");
        exact
            .write_slice(b"two")
            .as_ref()
            .expect("required fixture outcome");
        assert!(exact.try_commit().is_err());
    }
    assert_eq!(writer.len(), 0);

    let planned = writer.try_exact(4);
    let mut exact = planned.expect("second four-byte exact plan must fit spare storage");
    exact
        .write_slice(b"tw")
        .as_ref()
        .expect("required fixture outcome");
    exact
        .write_slice(b"o!")
        .as_ref()
        .expect("required fixture outcome");
    exact
        .try_commit()
        .as_ref()
        .expect("required fixture outcome");
    writer.commit();

    assert_eq!(buffer.as_slice(), b"two!");
}

#[test]
fn preserves_rejected_tail() {
    let pool = (pool::Pool::try_new(1, 8)).expect("the append fixture must allocate");
    let mut cursor = (pool.cursor()).expect("the append fixture owns a free slot");
    Sink::write_slice(&mut cursor, b"abcdef")
        .as_ref()
        .expect("required fixture outcome");
    cursor
        .try_consume(4)
        .as_ref()
        .expect("required fixture outcome");
    let position = cursor.as_ref().as_ptr();
    {
        let result = cursor.try_append(b"1234567");
        assert!(result.is_err(), "the retained suffix leaves only six bytes");
        let Err(error) = result else { return };
        let (required, room) = error.into_parts();
        assert_eq!((required.get(), room), (7, 6));
    }
    assert_eq!(cursor.as_ref(), b"ef");
    assert_eq!(cursor.as_ref().as_ptr(), position);
    let append =
        (cursor.try_append(b"123456")).expect("the exact remaining capacity must be admitted");
    append.commit();
    assert_eq!(cursor.as_ref(), b"ef123456");
}

#[test]
fn discards_unwritten_input() {
    let pool = (pool::Pool::try_new(1, 8)).expect("the cancellation fixture must allocate");
    let mut cursor = (pool.cursor()).expect("the cancellation fixture owns a free slot");
    Sink::write_slice(&mut cursor, b"abcdef")
        .as_ref()
        .expect("required fixture outcome");
    cursor
        .try_consume(4)
        .as_ref()
        .expect("required fixture outcome");
    assert!(
        cursor.try_append(b"123456").is_ok(),
        "compaction must admit the complete source"
    );
    assert_eq!(cursor.as_ref(), b"ef");
    assert_eq!(cursor.spare_len(), 6);
    Sink::write_slice(&mut cursor, b"gh")
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(cursor.as_ref(), b"efgh");
}

#[test]
fn publishes_borrowed_input() {
    let pool = (pool::Pool::try_new(1, 8)).expect("the borrowed fixture must allocate");
    let mut cursor = (pool.view()).expect("the borrowed fixture owns a free slot");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"abcdefgh"), b"abcdefgh".len());
        spare.commit();
    }
    cursor
        .try_consume(6)
        .as_ref()
        .expect("required fixture outcome");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"654321"), 6);
    }
    assert_eq!(cursor.as_ref(), b"gh");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"123456"), b"123456".len());
        spare.commit();
    }
    {
        let bytes = cursor.freeze();
        assert_eq!(bytes.as_slice(), b"gh123456");
        assert_eq!(pool.available(), 0);
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn bounds_admitted_writes() {
    use buffer::pool::Limit;

    let pool = (pool::Pool::try_new(1, 8)).expect("the bounded fixture must allocate");
    let mut cursor = (pool.cursor()).expect("the bounded fixture owns a free slot");
    Sink::write_slice(&mut cursor, b"abcdef")
        .as_ref()
        .expect("required fixture outcome");
    cursor
        .try_consume(4)
        .as_ref()
        .expect("required fixture outcome");
    let identity = String::from("frame");
    let origin = identity.as_ptr();
    let result = Limit::try_from(cursor, 7, identity);
    assert!(result.is_err(), "the retained suffix leaves six bytes");
    let Err((cursor, identity, error)) = result else {
        return;
    };
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (7, 6));
    assert_eq!(cursor.as_ref(), b"ef");
    assert_eq!(identity.as_ptr(), origin);
    let admitted = Limit::try_from(cursor, 5, identity);
    let mut limit = admitted
        .map_err(|rejected| (rejected.1, rejected.2))
        .expect("five bytes fit the retained cursor's six-byte spare region");
    limit
        .try_extend(b"ghi")
        .as_ref()
        .expect("required fixture outcome");
    let result = limit.try_extend(b"jkl");
    assert!(
        result.is_err(),
        "physical capacity does not authorize a larger quota"
    );
    let Err(error) = result else { return };
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (3, 2));
    assert_eq!(limit.remaining(), 2);
    limit
        .try_extend(b"jk")
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(limit.remaining(), 0);
    let (cursor, identity) = limit.into_parts();
    assert_eq!(cursor.as_ref(), b"efghijk");
    assert_eq!(identity.as_ptr(), origin);
}

#[test]
fn admits_before_acquiring() {
    use buffer::pool::{Error, Limit, Pool};

    let pool = (Pool::try_new(1, 8)).expect("the acquisition fixture must allocate");
    let identity = String::from("delivery");
    let origin = identity.as_ptr();
    let identity = {
        let held = (pool.cursor()).expect("the fixture owns a free slot");
        assert!(held.is_empty());
        let result = Limit::try_borrow(&pool, 9, identity);
        assert!(
            result.is_err(),
            "capacity rejection must precede slot exhaustion"
        );
        let Err(error) = result else { return };
        let (identity, error) = match error {
            (identity, Error::Capacity(error)) => Some((identity, error)),
            _ => None,
        }
        .expect("capacity rejection must precede slot exhaustion");
        let (required, room) = error.into_parts();
        assert_eq!((required.get(), room), (9, 8));
        let result = Limit::try_borrow(&pool, 8, identity);
        assert!(result.is_err(), "the sole slot is still held");
        let Err(error) = result else { return };
        let (identity,) = match error {
            (identity, Error::Empty) => Some((identity,)),
            _ => None,
        }
        .expect("the sole slot is still held");
        assert_eq!(identity.as_ptr(), origin);
        identity
    };
    {
        let mut limit = (Limit::try_borrow(&pool, 8, identity))
            .expect("the recycled slot must admit the exact quota");
        limit
            .try_extend(b"abcdefgh")
            .as_ref()
            .expect("required fixture outcome");
        let (cursor, identity) = limit.into_parts();
        assert_eq!(cursor.as_ref(), b"abcdefgh");
        assert_eq!(identity.as_ptr(), origin);
        assert_eq!(pool.available(), 0);
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn fills_fixed_bounds() {
    use buffer::{
        bytes::Prefix,
        pool::{Fill, Pool, Width},
    };

    let layout = (Width::<8>::new(1)).expect("the fixed fixture has one slot");
    let pool = (Pool::try_from(layout)).expect("the fixed fixture must allocate");
    let mut cursor = (pool.cursor()).expect("the fixed fixture owns a free slot");
    Sink::write_slice(&mut cursor, b"previous")
        .as_ref()
        .expect("required fixture outcome");
    cursor.consume_max(2);
    let fill = Fill::new(&mut cursor);
    let source = (Prefix::<4>::try_new(b"ab")).expect("the source is below its static maximum");
    let (fill, first) = fill.write::<4, 4>(source);
    fill.finish(b"cdef".into());
    first[0] = b'A';

    assert_eq!(cursor.as_ref(), b"Abcdef");
    assert_eq!(cursor.spare_len(), 2);
}

#[test]
fn appends_in_place() {
    let pool = (pool::Pool::try_new(1, 16)).expect("the append fixture must allocate its slot");
    let mut cursor = (pool.cursor()).expect("the fixture owns one free slot");
    let scope = Tracker::begin();
    write::Sink::write_slices(&mut cursor, [&b"head"[..], &[], b"body", b"tail"])
        .expect("the complete scatter source fits");
    cursor
        .try_consume(8)
        .expect("the initialized head and body must fit");
    let result = cursor.try_consume(usize::MAX);
    assert!(
        result.is_err(),
        "an oversized consume must preserve the initialized suffix"
    );
    let Err(error) = result else { return };
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (usize::MAX, 4));
    assert_eq!(cursor.as_ref(), b"tail");
    cursor.as_mut().copy_from_slice(b"TAIL");
    let append = (cursor.try_append(b"0123456789ab"))
        .expect("compaction must admit the exact remaining space");
    append.commit();
    assert_eq!(cursor.as_ref(), b"TAIL0123456789ab");
    assert_eq!(cursor.consume_max(usize::MAX), 16);
    assert!(cursor.as_ref().is_empty());
    assert!(cursor.as_mut().is_empty());
    cursor
        .write_slice(b"new")
        .expect("the empty cursor must fit three bytes");
    assert!(scope.finish() == Activity::default());
    assert_eq!(cursor.as_ref(), b"new");
}

#[test]
fn fills_in_place() {
    let layout = (pool::Width::<32>::new(1)).expect("the fixed-width fixture has one slot");
    let pool = (pool::Pool::try_from(layout)).expect("the fixed-width fixture must allocate");
    let mut cursor = (pool.cursor()).expect("the fixed-width fixture owns one free slot");
    let mut source = [b's'; 28];
    source[27] = b'T';
    let scope = Tracker::begin();
    for _ in 0..16 {
        let fill = pool::Fill::new(&mut cursor);
        let (fill, head) = fill.write::<4, 28>(b"head".into());
        fill.finish(bytes::Prefix::from(&source));
        head[0] = b'H';
    }
    assert!(scope.finish() == Activity::default());
    assert_eq!(&cursor.as_ref()[..4], b"Head");
    assert_eq!(&cursor.as_ref()[4..31], &[b's'; 27]);
    assert_eq!(cursor.as_ref()[31], b'T');
}

#[test]
fn retains_beyond_pool() {
    use o3::buffer::{Cursor as _, write::Sink as _};

    let scope;
    let retained = {
        let pool =
            (pool::Pool::try_new(1, 16)).expect("the retention fixture must allocate its slot");
        let mut cursor = (pool.view()).expect("the fixture owns one free slot");
        let mut spare = cursor.spare();
        spare
            .write_slice(b"payload")
            .expect("the payload must fit the borrowed slot");
        spare.commit();
        let source = cursor.freeze();
        let pointer = source.as_slice()[1..6].as_ptr();
        scope = Tracker::begin();
        let plan = (source.try_get(1..6)).expect("the retained range lies inside the source");
        let retained = (plan.try_own()).expect("the first independent edge must fit");
        assert_eq!(retained.as_slice().as_ptr(), pointer);
        retained
    };
    assert!(scope.finish() == Activity::default());
    let ((), freed) = Tracker::activity(move || assert_eq!(retained.as_slice(), b"ayloa"));
    assert_eq!(freed.allocations, 0);
    assert_eq!(freed.deallocations, 1);
}
