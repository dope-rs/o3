pub mod common;

use std::{hint, num, pin};

use common::alloc::system::{Activity, Tracker};
use o3::{
    alloc,
    buffer::{
        bytes::window::Snapshot,
        resident, segments,
        storage::{self, provided},
        write::{Sink as _, consume::Consume},
    },
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn freeze_retains_extent() {
    let mut window =
        Snapshot::<8>::try_with(num::NonZero::new(4).expect("capacity")).expect("window");
    window.write_slice(b"abcd").expect("prefix");
    window.try_consume(3).expect("consume");
    assert_eq!(window.freeze().as_ref(), b"d");
    let mut window =
        Snapshot::<8>::try_with(num::NonZero::new(4).expect("capacity")).expect("empty window");
    let before = (window.as_ref().as_ptr(), window.spare().remaining());
    let hot = Tracker::begin();
    window.write_slices([]).expect("empty batch");
    window.write_slices([&[][..]; 2]).expect("empty slices");
    assert_eq!(hot.finish(), Activity::default());
    assert_eq!(
        (window.as_ref().as_ptr(), window.spare().remaining()),
        before
    );
    assert!(window.freeze().is_empty());
}

#[test]
fn spare_preserves_views() {
    let mut arena =
        provided::Arena::<[u8; 2]>::try_with(num::NonZero::<u32>::MIN.saturating_add(1), || {
            Ok::<(), alloc::Error>(())
        })
        .expect("provided arena");
    let mut domain = resident::Domain::new();
    arena.scope(pin::Pin::new(&mut domain), |token, arena, domain| {
        let budget = domain
            .get_mut()
            .try_budget(4, token.proof())
            .expect("budget");
        let pool = segments::Pool::try_new(segments::Plan {
            segments: 2,
            chains: 1,
        })
        .expect("segment pool");
        let mut source = pool.scoped();
        for bytes in [*b"cd", *b"ef"] {
            let owner = arena
                .vacant()
                .expect("vacant")
                .try_insert(bytes, 2, |bytes| bytes.as_mut_slice())
                .expect("source")
                .try_account(&budget)
                .expect("account");
            source
                .try_push(resident::Shared::from(owner))
                .expect("segment");
        }
        let mut window =
            Snapshot::<8>::try_with(num::NonZero::new(6).expect("capacity")).expect("window");
        let hot = Tracker::begin();
        let mut empty = Snapshot::<8>::new();
        let before = (empty.as_ref().as_ptr(), empty.spare().remaining());
        empty.write_slices([]).expect("empty batch");
        empty.write_slices([&[][..]; 2]).expect("empty slices");
        assert_eq!((empty.as_ref().as_ptr(), empty.spare().remaining()), before);
        let error = empty.spare().write_slice(b"x").expect_err("no allocation");
        assert_eq!(error.into_parts(), (num::NonZero::<usize>::MIN, 0));
        empty.spare().commit();
        assert!(empty.freeze().is_empty());
        {
            let mut spare = window.spare();
            spare.write_slice(b"ab").expect("prefix");
            spare.commit();
        }
        let prefix = window.try_snapshot().expect("retain").expect("prefix");
        window.clear();
        {
            let mut spare = window.spare();
            spare.write_slice(b"x").expect("uncommitted suffix");
            let error = source
                .try_drain(&mut spare)
                .expect_err("complete chain exceeds suffix");
            assert_eq!(
                error.into_parts(),
                (num::NonZero::new(4).expect("length"), 3)
            );
            assert_eq!(spare.len(), 1);
            assert_eq!(
                (source.len(), source.count().into_raw(), budget.resident()),
                (4, 2, 4)
            );
            assert!(!arena.has_returned());
        }
        assert!(window.is_empty());
        {
            let mut spare = window.spare();
            source.try_drain(&mut spare).expect("complete chain");
            spare.commit();
        }
        assert!(source.is_empty());
        assert_eq!(budget.resident(), 0);
        assert_eq!(arena.pop_returned(), Some(*b"cd"));
        assert_eq!(arena.pop_returned(), Some(*b"ef"));
        assert_eq!(arena.pop_returned(), None);
        {
            let mut spare = window.spare();
            source.try_drain(&mut spare).expect("empty chain");
            assert_eq!(spare.len(), 0);
            spare.commit();
        }
        assert_eq!(window.spare().remaining(), 0);
        let suffix = window.freeze();
        assert_eq!(prefix.as_ref(), b"ab");
        assert_eq!(suffix.as_ref(), b"cdef");
        assert_eq!(suffix.as_ptr(), prefix.as_ptr().wrapping_add(2));
        assert_eq!(hot.finish(), Activity::default());
    });
}

#[test]
fn batch_failure_is_inert() {
    let mut window = Snapshot::<8>::try_with(num::NonZero::new(4).expect("capacity"))
        .expect("window allocation");
    window.write_slice(b"abcd").expect("initial prefix");
    let retained = window.try_snapshot().expect("retain").expect("prefix");
    window.try_consume(3).expect("consumed prefix");
    let before = (
        window.len(),
        window.as_ref().as_ptr(),
        window.spare().remaining(),
    );
    let ((result, next), activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            let result = window.write_slices([b"efgh".as_slice(), b"ijkl"]);
            let next = <Box<u8> as alloc::BoxExt<u8>>::try_new(0);
            (result, next)
        })
    });
    assert!(matches!(result, Err(storage::AllocError::Capacity(error))
        if error.into_parts() == (num::NonZero::<usize>::MIN.saturating_add(3), 3)));
    assert!(matches!(next, Err(alloc::Exhausted)));
    assert_eq!(activity, [0; 4]);
    assert_eq!(
        (
            window.len(),
            window.as_ref().as_ptr(),
            window.spare().remaining()
        ),
        before
    );
    assert_eq!(window.as_ref(), b"d");
    assert_eq!(retained.as_ref(), b"abcd");
    window
        .write_slices([b"e".as_slice(), b"f"])
        .expect("whole batch");
    assert_eq!(window.as_ref(), b"def");
}

#[test]
fn keeps_exhausted_batch() {
    for shared in [false, true] {
        let mut window = Snapshot::<8>::try_with(num::NonZero::new(4).expect("capacity"))
            .expect("window allocation");
        window.write_slice(b"abcd").expect("initial prefix");
        let retained = shared.then(|| window.try_snapshot().expect("retain").expect("prefix"));
        let before = (
            window.len(),
            window.as_ref().as_ptr(),
            window.spare().remaining(),
        );
        let (result, activity) = Tracker::measure(|| {
            Tracker::fail_after(0, || window.write_slices([b"e".as_slice(), b"fgh"]))
        });
        assert!(matches!(
            result,
            Err(storage::AllocError::Exhausted(alloc::Exhausted))
        ));
        assert_eq!(activity, [0; 4]);
        assert_eq!(
            (
                window.len(),
                window.as_ref().as_ptr(),
                window.spare().remaining()
            ),
            before
        );
        assert_eq!(window.as_ref(), b"abcd");
        if let Some(retained) = retained {
            assert_eq!(retained.as_ref(), b"abcd");
        }
    }
}

#[test]
fn grows_initialized_batch() {
    for (allowed, spare) in [(1, 7), (0, 0)] {
        let mut window =
            Snapshot::<16>::try_copy(b"abc", num::NonZero::<usize>::MIN.saturating_add(7))
                .expect("partially initialized allocation");
        let (result, resizes) = Tracker::resizes(|| {
            Tracker::fail_after(allowed, || window.write_slices([&b"de"[..], b"fghi"]))
        });
        result.expect("growth preserves the admitted prefix and batch");
        assert_eq!(resizes, 1);
        assert_eq!(window.as_ref(), b"abcdefghi");
        assert_eq!(window.spare().remaining(), spare);
    }
}

#[test]
fn compacts_batch() {
    let mut window =
        Snapshot::<8>::try_copy(b"abcdefg", num::NonZero::<usize>::MIN.saturating_add(7))
            .expect("window allocation");
    window.try_consume(2).expect("consumed prefix");
    let (result, activity) = Tracker::measure(|| window.write_slices([&b"h"[..], b"i"]));
    result.expect("complete batch fits after compaction");
    assert_eq!(activity, [0; 4]);
    assert_eq!(window.as_ref(), b"cdefghi");
    assert_eq!(window.spare().remaining(), 1);
}

#[test]
fn reuses_batch_spare() {
    let bytes = b"cde".as_slice();
    for split in 0..=bytes.len() {
        let split = hint::black_box(split);
        let mut window = Snapshot::<8>::try_with(num::NonZero::new(6).expect("capacity"))
            .expect("window allocation");
        window.write_slice(b"ab").expect("initial prefix");
        let retained = window.try_snapshot().expect("retain").expect("prefix");
        window.try_consume(1).expect("consumed prefix");
        let pointer = window.as_ref().as_ptr();
        let (result, activity) =
            Tracker::measure(|| window.write_slices([&bytes[..split], &bytes[split..]]));
        result.expect("whole batch");
        assert_eq!(activity, [0; 4]);
        assert_eq!(window.as_ref(), b"bcde");
        assert_eq!(window.as_ref().as_ptr(), pointer);
        assert_eq!(window.spare().remaining(), 1);
        assert_eq!(retained.as_ref(), b"ab");
    }
}

#[test]
fn appends_and_consumes() {
    let mut buf =
        Snapshot::<{ 1 << 20 }>::try_with(num::NonZero::<usize>::MIN.saturating_add(16383))
            .expect("asserted snapshot allocation");
    assert!(buf.is_empty());
    buf.write_slice(b"hello ")
        .as_ref()
        .expect("required fixture outcome");
    buf.write_slice(b"world")
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(buf.len(), 11);
    assert_eq!(
        buf.try_snapshot()
            .expect("asserted retained edge")
            .expect("written bytes produce a snapshot")
            .as_slice(),
        b"hello world"
    );

    let () = (buf.try_consume(6)).expect("the written six-byte snapshot prefix must be consumable");
    assert_eq!(
        buf.try_snapshot()
            .expect("asserted retained edge")
            .expect("remaining bytes produce a snapshot")
            .as_slice(),
        b"world"
    );

    let chunk = vec![7u8; 20 * 1024];
    buf.write_slice(&chunk)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(buf.len(), 5 + chunk.len());
    assert_eq!(buf.consume_max(usize::MAX), 5 + chunk.len());
    assert!(buf.is_empty());
    assert!(
        buf.try_snapshot()
            .expect("asserted retained edge")
            .is_none()
    );

    let mut small =
        Snapshot::<{ 16 * 1024 }>::try_with(num::NonZero::<usize>::MIN.saturating_add(16383))
            .expect("asserted snapshot allocation");
    assert!(small.write_slice(&vec![0u8; 16 * 1024 + 1]).is_err());
}

#[test]
fn retains_shared_ranges() {
    let mut buf =
        Snapshot::<{ 1 << 20 }>::try_with(num::NonZero::<usize>::MIN.saturating_add(16383))
            .expect("asserted snapshot allocation");
    buf.write_slice(b"before")
        .as_ref()
        .expect("required fixture outcome");
    let snap = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("written prefix produces a snapshot");
    buf.write_slice(b"!")
        .as_ref()
        .expect("required fixture outcome");
    let now = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("extended bytes produce a snapshot");
    assert_eq!(snap.as_slice(), b"before");
    assert_eq!(now.as_slice(), b"before!");
    assert_eq!(snap.as_ptr(), now.as_ptr());

    let mut buf =
        Snapshot::<{ 1 << 20 }>::try_with(num::NonZero::<usize>::MIN.saturating_add(16383))
            .expect("asserted snapshot allocation");
    buf.write_slice(b"before")
        .as_ref()
        .expect("required fixture outcome");
    let snap = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("written prefix produces a snapshot");
    assert_eq!(buf.consume_max(usize::MAX), 6);
    buf.write_slice(b"later")
        .as_ref()
        .expect("required fixture outcome");
    let later = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("replacement bytes produce a snapshot");
    assert_eq!(snap.as_slice(), b"before");
    assert_eq!(later.as_slice(), b"later");
    assert_ne!(snap.as_ptr(), later.as_ptr());

    let mut buf =
        Snapshot::<{ 1 << 20 }>::try_with(num::NonZero::<usize>::MIN.saturating_add(16383))
            .expect("asserted snapshot allocation");
    buf.write_slice(b"discardlive")
        .as_ref()
        .expect("required fixture outcome");
    let () =
        (buf.try_consume(7)).expect("the seven-byte discarded snapshot prefix must be consumable");
    let snap = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("live suffix produces a snapshot");
    buf.write_slice(b"!")
        .as_ref()
        .expect("required fixture outcome");
    let now = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("extended suffix produces a snapshot");
    assert_eq!(snap.as_slice(), b"live");
    assert_eq!(now.as_slice(), b"live!");
    assert_eq!(snap.as_ptr(), now.as_ptr());
}

#[test]
fn grows_retained_prefix() {
    let mut buf = Snapshot::<64>::try_with(num::NonZero::<usize>::MIN.saturating_add(7))
        .expect("asserted snapshot allocation");
    buf.write_slice(b"prefix")
        .as_ref()
        .expect("required fixture outcome");
    let held = buf
        .try_snapshot()
        .expect("asserted retained edge")
        .expect("written prefix produces a snapshot");
    let alloc = held.as_ptr();

    buf.write_slice(held.as_slice())
        .as_ref()
        .expect("required fixture outcome");

    let current = buf
        .try_snapshot()
        .expect("retained edge")
        .expect("the appended owner retains the complete initialized range");
    assert_eq!(current.as_slice(), b"prefixprefix");
    assert_ne!(current.as_slice().as_ptr(), alloc);
    assert_eq!(held.as_slice(), b"prefix");
}

#[test]
fn transfers_reserved_tail() {
    let cap = num::NonZero::<usize>::MIN.saturating_add(7);
    let (last, cold) = {
        let (owner, first, cold) = {
            let (built, cold) = Tracker::measure(|| Snapshot::<16>::try_with(cap));
            let mut window = (built).expect("the reserved window must allocate");
            assert_eq!(cold[0], 1);
            let (written, hot) = Tracker::measure(|| window.write_slice(b"abcdef"));
            written.as_ref().expect("required fixture outcome");
            assert_eq!(hot, [0; 4]);
            window
                .try_consume(2)
                .as_ref()
                .expect("required fixture outcome");
            let (peer, hot) = Tracker::measure(|| window.try_snapshot());
            let peer = peer
                .expect("snapshot retention must fit")
                .expect("the retained suffix must exist");
            assert_eq!(hot, [0; 4]);
            assert_eq!(peer.as_slice(), b"cdef");
            let address = peer.as_ptr();
            let (owner, transfer) = Tracker::measure(|| window.into_shared());
            assert_eq!(transfer, [0; 4]);
            let owner = (owner).expect("the nonempty source transfers its owner");
            assert_eq!(owner.as_ptr(), address);
            (owner, Tracker::begin(), cold)
        };
        assert_eq!(first.finish(), Activity::default());
        assert_eq!(owner.as_slice(), b"cdef");
        (Tracker::begin(), cold)
    };
    assert_eq!(
        last.finish(),
        Activity {
            deallocations: cold[0],
            deallocated_bytes: cold[1],
            ..Default::default()
        }
    );
}

#[test]
fn rejects_capacity() {
    let cap = num::NonZero::<usize>::MIN.saturating_add(8);
    let (built, activity) = Tracker::measure(|| Snapshot::<8>::try_with(cap));
    assert_eq!(activity, [0; 4]);
    let error = match built {
        Err(storage::AllocError::Capacity(error)) => Some(error),
        _ => None,
    }
    .expect("the exact ceiling rejects the ninth byte");
    let (required, capacity) = error.into_parts();
    assert_eq!(required.get(), 9);
    assert_eq!(capacity, 8);
    let (result, allocs) = Tracker::count(|| Snapshot::<0>::try_with(num::NonZero::<usize>::MIN));
    assert!(matches!(result, Err(storage::AllocError::Capacity(error))
            if error.into_parts() == (num::NonZero::<usize>::MIN, 0)));
    assert_eq!(allocs, 0);
}

#[test]
fn initializes_capacity() {
    let floor = num::NonZero::<usize>::MIN.saturating_add(3);
    for (src, spare) in [(&b""[..], 4), (&b"abc"[..], 1), (&b"abcde"[..], 3)] {
        let (built, allocs) = Tracker::count(|| Snapshot::<16>::try_copy(src, floor));
        let mut window = built.expect("initial copy");
        assert_eq!(allocs, 1);
        assert_eq!(window.as_ref(), src);
        assert_eq!(window.spare().remaining(), spare);
    }
    let (built, allocs) =
        Tracker::count(|| Tracker::fail_after(0, || Snapshot::<16>::try_copy(b"abcde", floor)));
    let mut window = built.expect("required capacity fallback");
    assert_eq!(allocs, 1);
    assert_eq!(window.as_ref(), b"abcde");
    assert_eq!(window.spare().remaining(), 0);
    let (built, allocs) =
        Tracker::count(|| Tracker::fail_after(0, || Snapshot::<16>::try_copy(b"abcdefgh", floor)));
    assert!(matches!(built, Err(storage::AllocError::Exhausted(_))));
    assert_eq!(allocs, 0);
    let over = num::NonZero::<usize>::MIN.saturating_add(16);
    for (src, initial) in [(&b""[..], over), (&b"abcdefghijklmnopq"[..], floor)] {
        let (built, activity) = Tracker::measure(|| Snapshot::<16>::try_copy(src, initial));
        assert!(matches!(built, Err(storage::AllocError::Capacity(error))
            if error.into_parts() == (over, 16)));
        assert_eq!(activity, [0; 4]);
    }
}

#[test]
fn exhaustion_is_inert() {
    let cap = num::NonZero::<usize>::MIN.saturating_add(31);
    let (snapshot, trace) =
        Tracker::activity(|| Tracker::fail_after(0, || Snapshot::<64>::try_with(cap)));
    assert!(matches!(
        snapshot,
        Err(storage::AllocError::Exhausted(alloc::Exhausted))
    ));
    assert!(trace == Activity::default());
}
