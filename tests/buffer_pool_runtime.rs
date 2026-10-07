pub mod common;

use common::alloc::system::Tracker;
use o3::buffer::{
    Residency as _,
    pool::{self, CreateError, LayoutError, borrowed, state},
    write::SpareExt as _,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn owns_single_allocation() {
    let mut lease =
        (pool::Span::try_new(8)).expect("the standalone eight-byte lease must allocate");
    assert_eq!(lease.capacity().get(), 8);
    lease
        .try_append(b"payload")
        .expect("required fixture outcome")
        .commit();
    let frozen = lease.freeze();
    assert_eq!(frozen.as_ref(), b"payload");
    assert!(matches!(
        pool::Span::try_new(0),
        Err(CreateError::Layout(LayoutError::ZeroCapacity))
    ));
}

#[test]
fn prefix_keeps_spare() {
    use o3::buffer::write::Sink as _;

    let pool = pool::Pool::try_new(1, 12).expect("one editable slot");
    {
        let mut lease = pool.acquire().expect("the slot is available");
        lease
            .try_append(b"abcdef")
            .expect("initial bytes fit")
            .commit();
        let pointer = lease.as_slice().as_ptr();

        let result = lease.try_prefix::<2>(5);
        assert!(result.is_err(), "prefix crosses the initialized end");
        let Err(error) = result else { return };
        assert_eq!((error.offset, error.width, error.len), (5, 2, 6));
        let result = lease.try_prefix::<0>(7);
        assert!(
            result.is_err(),
            "empty prefix still requires a valid offset"
        );
        let Err(error) = result else { return };
        assert_eq!((error.offset, error.width, error.len), (7, 0, 6));
        assert_eq!(lease.as_slice(), b"abcdef");

        {
            let mut edit = lease.try_prefix::<2>(1).expect("interior prefix fits");
            edit.prefix().copy_from_slice(b"XY");
            edit.write_slice(b"gh").expect("staged bytes fit");
            let result = edit.write_slice(b"too-long");
            assert!(
                result.is_err(),
                "staged suffix exceeds the remaining spare capacity"
            );
            let Err(error) = result else { return };
            let (required, spare) = error.into_parts();
            assert_eq!((required.get(), spare), (8, 4));
            assert_eq!(edit.len(), 2);
            assert_eq!(edit.prefix(), b"XY");
        }
        assert_eq!(lease.as_slice(), b"aXYdef");

        let mut edit = lease
            .try_prefix::<2>(4)
            .expect("prefix ends at the initialized boundary");
        edit.prefix().copy_from_slice(b"QR");
        edit.write_slice(b"ij")
            .expect("replacement staged bytes fit");
        assert_eq!(edit.prefix(), b"QR");
        edit.commit();
        assert_eq!(lease.as_slice(), b"aXYdQRij");

        let mut edit = lease
            .try_prefix::<0>(8)
            .expect("empty prefix at the initialized end");
        edit.write_slice(b"KL").expect("tail still has capacity");
        edit.commit();
        assert_eq!(lease.as_slice(), b"aXYdQRijKL");
        assert_eq!(lease.as_slice().as_ptr(), pointer);
        assert_eq!(pool.available(), 0);
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn copies_with_ownership() {
    let pool = (pool::Pool::try_new(1, 4)).expect("the single-slot copy pool must allocate");
    let result = pool.try_copy(b"large");
    assert!(
        result.is_err(),
        "a five-byte source must not fit the four-byte slot"
    );
    let Err(error) = result else { return };
    assert_eq!(
        {
            let (required, spare) = error.into_parts();
            (required.get(), spare)
        },
        (5, 4)
    );
    assert_eq!(pool.available(), 1);

    {
        let (frozen,) = (((pool.try_copy(b"body"))
            .expect("the available four-byte slot must accept the body"))
        .expect("the available four-byte slot must accept the body"),);
        assert_eq!(frozen.as_ref(), b"body");
        assert_eq!(pool.available(), 0);
        let next = (pool.try_copy(b"next"))
            .expect("pool exhaustion must not become an allocation failure");
        assert!(next.is_none());
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn returns_after_last_clone() {
    let pool = (pool::Pool::try_new(1, 16)).expect("the clone-return pool must allocate");
    let mut lease = pool.acquire().expect("one slot is available");
    lease
        .try_append(b"body")
        .expect("required fixture outcome")
        .commit();
    let clone = {
        let body = lease.freeze().into_shared();
        let clone = body.try_clone().expect("asserted retained edge");
        assert!(pool.acquire().is_none());
        clone
    };
    assert!(pool.acquire().is_none());
    {
        let retained = clone;
        assert_eq!(retained.as_ref(), b"body");
    }
    assert!(pool.acquire().is_some());
}

#[test]
fn outlives_pool() {
    let body = {
        let pool = (pool::Pool::try_new(1, 8)).expect("the detached-body pool must allocate");
        let mut lease = pool.acquire().expect("one slot is available");
        lease
            .try_append(b"abc")
            .expect("required fixture outcome")
            .commit();
        lease.freeze()
    };
    assert_eq!(body.as_ref(), b"abc");
}

#[test]
fn transfers_cursor_owner() {
    let pool = (pool::Pool::try_new(1, 16)).expect("the cursor-transfer pool must allocate");
    let mut cursor = pool.view().expect("borrowed slot");
    {
        let mut spare = cursor.spare();
        assert_eq!(spare.write(b"shared-body"), b"shared-body".len());
        spare.commit();
    }
    let source = cursor.as_ref().as_ptr();
    let peer = {
        let shared = cursor.freeze();
        let peer = shared.try_clone().expect("asserted retained edge");
        assert_eq!(shared.as_slice(), b"shared-body");
        assert_eq!(shared.as_slice().as_ptr(), source);
        assert_eq!(shared.resident(), 16);
        assert_eq!(pool.available(), 0);
        peer
    };
    assert_eq!(pool.available(), 0);
    {
        let peer = peer;
        assert_eq!(peer.as_slice(), b"shared-body");
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn builds_from_layout() {
    let layout =
        (pool::Layout::new(2, 32)).expect("the two-slot thirty-two-byte layout must be valid");
    assert_eq!(layout.slots(), 2);

    let first = (pool::Pool::<state::Uninit>::try_from(layout))
        .expect("the uninitialized pool must allocate from the validated layout");
    let second = (pool::Pool::<state::Initialized>::try_from(layout))
        .expect("the initialized pool must allocate from the validated layout");
    assert_eq!(first.available(), 2);
    assert_eq!(second.available(), 2);
    assert_eq!(first.capacity().get(), 32);
    assert_eq!(second.capacity().get(), 32);
}

#[test]
fn keeps_compact_layouts() {
    use std::mem::size_of;

    assert_eq!(
        size_of::<pool::Span<pool::owners::Plain<pool::Layout>, pool::Owned, pool::Frozen>>(),
        2 * size_of::<usize>()
    );
}

#[test]
fn source_reuses_range() {
    let pool = pool::Pool::<state::Initialized>::try_new(1, 32);
    let pool = pool.expect("required fixture outcome");
    let source = borrowed::Source::try_new(&pool, 16, 8);
    let source = source.expect("required fixture outcome");
    let ((), counts) = Tracker::usage(|| {
        let mut previous = None;
        for _ in 0..2 {
            {
                let slot = source.acquire();
                let mut slot = slot.expect("assertion failed: slot.is_some()");
                let pointer = slot.as_slice().as_ptr();
                if let Some(previous) = previous {
                    assert_eq!(pointer, previous);
                }
                previous = Some(pointer);
                assert_eq!(slot.len(), 16);
                assert_eq!(slot.resident(), 32);
                slot.as_mut_slice().fill(0x42);
                assert!(source.acquire().is_none());
                let frozen = slot.freeze();
                assert_eq!(frozen.as_slice(), [0x42; 16]);
                assert_eq!(frozen.as_slice().as_ptr(), pointer);
                assert_eq!(pool.available(), 0);
            }
            assert_eq!(pool.available(), 1);
        }
    });
    assert_eq!(counts, (0, 0));
}

#[test]
fn rejects_exact_extent() {
    let pool = pool::Pool::<state::Initialized>::try_new(1, 32);
    let pool = pool.expect("required fixture outcome");
    for (length, suffix) in [(33, 0), (16, 17), (0, usize::MAX)] {
        let result = borrowed::Source::try_new(&pool, length, suffix);
        assert!(matches!(result, Err(borrowed::Error::Capacity {
            length: actual, suffix: tail, capacity,
        }) if actual == length && tail == suffix && capacity.get() == 32));
        assert_eq!(pool.available(), 1);
    }
}

#[test]
fn reject_keeps_payload() {
    let pool = pool::Pool::<state::Initialized>::try_new(1, 32);
    let pool = pool.expect("required fixture outcome");
    let source = borrowed::Source::try_new(&pool, 16, 8);
    let source = source.expect("required fixture outcome");
    let slot = source.acquire();
    let mut slot = slot.expect("assertion failed: slot.is_some()");
    slot.as_mut_slice().fill(0x42);
    let base = slot.as_slice().as_ptr();
    let ((), counts) = Tracker::usage(|| {
        let rejected = slot.try_frame(&[0x41; 9], 8);
        assert!(rejected.is_err());
        let Err((slot, error)) = rejected else { return };
        assert_eq!(
            error,
            borrowed::Error::Prefix {
                requested: 9,
                available: 8
            }
        );
        let rejected = slot.try_frame(&[0x41; 8], 9);
        assert!(rejected.is_err());
        let Err((slot, error)) = rejected else { return };
        assert_eq!(
            error,
            borrowed::Error::Suffix {
                requested: 9,
                available: 8
            }
        );
        assert_eq!(slot.as_slice().as_ptr(), base);
        assert_eq!(pool.available(), 0);
        let frame = slot.try_frame(&[0x41; 8], 8);
        match frame {
            Ok(mut frame) => {
                assert_eq!(frame.len(), 32);
                assert_eq!(frame.payload_end(), 24);
                assert_eq!(frame.prefix_mut(), [0x41; 8]);
                assert_eq!(&frame.as_mut_slice()[8..24], [0x42; 16]);
                let slot = frame.into_slice();
                assert_eq!(slot.len(), 16);
                assert_eq!(slot.as_slice().as_ptr(), base);
                assert_eq!(slot.as_slice(), [0x42; 16]);
                let frame = slot.try_frame(&[0x41; 4], 8);
                match frame {
                    Ok(frame) => {
                        {
                            let frozen = frame.freeze();
                            assert_eq!(frozen.as_slice().len(), 28);
                            assert_eq!(&frozen.as_slice()[..4], [0x41; 4]);
                            assert_eq!(frozen.as_slice().as_ptr().wrapping_add(4), base);
                        }
                        assert_eq!(pool.available(), 1);
                    }
                    outcome => {
                        assert!(outcome.is_ok(), "required fixture outcome");
                    }
                }
            }
            outcome => {
                assert!(outcome.is_ok(), "required fixture outcome");
            }
        }
    });
    assert_eq!(counts, (0, 0));
}

#[test]
fn empty_releases_slot() {
    let pool = pool::Pool::<state::Initialized>::try_new(1, 32);
    let pool = pool.expect("required fixture outcome");
    let source = borrowed::Source::try_new(&pool, 0, 0);
    let source = source.expect("required fixture outcome");
    {
        let selected = source.acquire();
        let mut selected = selected.expect("assertion failed: selected.is_some()");
        assert!(selected.as_mut_slice().is_empty());
        assert_eq!(selected.resident(), 32);
        let frozen = selected.freeze();
        assert!(frozen.as_slice().is_empty());
        assert_eq!(pool.available(), 1);
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn partition() {
    let pool = pool::Pool::<state::Initialized>::try_new(1, 32);
    let pool = pool.expect("required fixture outcome");
    let source = borrowed::Source::try_new(&pool, 16, 8);
    let source = source.expect("required fixture outcome");
    let split = source.into_split().try_suffix(17);
    assert!(matches!(
        split,
        Err(borrowed::Error::Partition {
            suffix: 17,
            length: 16
        })
    ));
    assert_eq!(pool.available(), 1);
    let source = borrowed::Source::try_new(&pool, 16, 8);
    let source = source.expect("required fixture outcome");
    let split = source.into_split().try_suffix(4);
    let split = split.expect("required fixture outcome");
    let ((), counts) = Tracker::usage(|| {
        for _ in 0..2 {
            {
                let frame = split.acquire();
                let mut frame = frame.expect("assertion failed: frame.is_some()");
                let pointer = frame.as_mut_slice().as_ptr();
                frame.prefix_mut().copy_from_slice(&[0x41; 12]);
                frame.payload_mut().copy_from_slice(&[0x42; 4]);
                let joined = frame.join();
                assert_eq!(joined.len(), 16);
                assert_eq!(joined.as_slice().as_ptr(), pointer);
                assert_eq!(&joined.as_slice()[..12], [0x41; 12]);
                assert_eq!(&joined.as_slice()[12..], [0x42; 4]);
            }
            assert_eq!(pool.available(), 1);
        }
    });
    assert_eq!(counts, (0, 0));
}
