pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{
    cell::Cell,
    collections::hash_map::DefaultHasher,
    hash::{Hash as _, Hasher as _},
    hint,
    mem::size_of,
    num::NonZero,
    pin::Pin,
};

use common::{
    alloc::system::Tracker,
    buffer::{Reject, provided::Item},
};
use o3::{
    alloc,
    buffer::{
        bytes::Prefix,
        storage::{
            inline::{Bytes, CAPACITY, Fill},
            provided,
        },
        write::Sink as _,
    },
};

#[test]
fn fills_at_capacity() {
    let bytes = [b'x'; CAPACITY];
    let mut inline = Bytes::<CAPACITY>::new();
    Fill::new(&mut inline).finish((&bytes).into());

    assert_eq!(inline.as_ref(), bytes);
    assert_eq!(size_of::<Bytes>(), CAPACITY + 1);
    assert!(Bytes::<CAPACITY>::from_slice(&[0; CAPACITY + 1]).is_err());

    let error = inline.write_byte(0).expect_err("full inline buffer");
    let (required, available) = error.into_parts();
    assert_eq!((required.get(), available), (1, 0));
}

#[test]
fn converts_bounded_prefix() {
    let prefix =
        (Prefix::<3>::try_new(b"abc")).expect("the exact-capacity prefix must be accepted");
    let inline = Bytes::<3>::from(prefix);
    assert_eq!(inline.as_ref(), b"abc");

    let error = Prefix::<2>::try_new(b"abc").expect_err("oversized prefix");
    assert_eq!(
        {
            let (required, available) = error.into_parts();
            (required.get(), available)
        },
        (3, 2)
    );

    let bytes = *b"0123456789abcdef\r\n";
    for skip in 0..=bytes.len() {
        let mut source = bytes.into_iter();
        if skip != 0 {
            assert!(source.nth(skip - 1).is_some());
        }
        let (value, activity) = Tracker::measure(|| Bytes::<18, u16>::from(Prefix::from(&source)));
        assert_eq!(activity, [0; 4]);
        assert_eq!(value.as_ref(), &bytes[skip..]);
    }
}

#[test]
fn uses_typed_capacity() {
    let mut inline = (Bytes::<4>::from_slice(b"ab"))
        .expect("the two-byte input must fit the typed four-byte capacity");
    inline
        .write_slice(b"cd")
        .as_ref()
        .expect("required fixture outcome");

    assert_eq!(inline.as_ref(), b"abcd");
    assert_eq!(size_of::<Bytes<4>>(), 5);

    let error = inline.write_slice(b"e").expect_err("full inline buffer");
    assert_eq!(
        {
            let (required, available) = error.into_parts();
            (required.get(), available)
        },
        (1, 0)
    );
}

#[test]
fn compares_active_prefix() {
    let mut first =
        (Bytes::<8>::from_slice(b"abc")).expect("the first active-prefix input must fit");
    let mut second =
        (Bytes::<8>::from_slice(b"axy")).expect("the second active-prefix input must fit");
    first.truncate(1);
    second.truncate(1);

    let copied = first;
    assert_eq!(copied.as_ref(), b"a");
    assert!(first == second);

    let mut h1 = DefaultHasher::new();
    first.hash(&mut h1);
    let mut h2 = DefaultHasher::new();
    second.hash(&mut h2);
    assert_eq!(h1.finish(), h2.finish());
    assert_ne!(&*first.backing_mut(), &*second.backing_mut());
}

#[test]
fn publishes_exact_region() {
    let mut bytes = Bytes::<8>::new();
    let append = (bytes.try_reserve(2)).expect("the initial two-byte region must fit");
    append.write(|output| output.copy_from_slice(b"ab"));
    assert_eq!(bytes.as_ref(), b"ab");

    let append = (bytes.try_reserve(2)).expect("the rejected two-byte region must fit");
    let reject = append.try_write(|output| {
        output.copy_from_slice(b"xx");
        Err::<(), _>(Reject::Requested)
    });
    assert!(reject == Err(Reject::Requested));
    assert_eq!(bytes.as_ref(), b"ab");

    {
        let mut append = bytes.try_reserve(2).expect("the abandoned region must fit");
        append.as_mut().copy_from_slice(b"zz");
    }
    assert_eq!(bytes.as_ref(), b"ab");

    let mut append = bytes.try_reserve(2).expect("the committed region must fit");
    append.as_mut().copy_from_slice(b"cd");
    append.commit();
    assert_eq!(bytes.as_ref(), b"abcd");
}

#[test]
fn zeroes_full_backing() {
    let mut bytes = (Bytes::<6>::from_slice(b"secret"))
        .expect("the six-byte secret must fill the inline backing exactly");
    bytes.truncate(2);
    {
        (bytes.write_slice(&[0; 3]))
            .as_ref()
            .expect("required fixture outcome");
    };
    assert_eq!(bytes.as_ref(), b"se\0\0\0");
    assert_eq!(bytes.available(), 1);

    bytes.backing_mut().fill(0);
    assert_eq!(&*bytes.backing_mut(), &[0; 6]);
    assert_eq!(bytes.len(), 5);
    bytes.clear();
    assert!(bytes.is_empty());
}

#[test]
fn keeps_u16_layout() {
    let mut inline = Bytes::<514, u16>::new();
    {
        (inline.write_slice(&[b'x'; 513]))
            .as_ref()
            .expect("required fixture outcome");
    };
    inline
        .write_byte(b'y')
        .as_ref()
        .expect("required fixture outcome");

    assert_eq!(inline.len(), 514);
    assert!(!inline.is_empty());
    assert_eq!(inline.as_ref()[513], b'y');
    assert_eq!(size_of::<Bytes<514, u16>>(), 516);
    assert!(inline.write_byte(0).is_err());
}

#[test]
fn alloc_skips_context() {
    let called = Cell::new(false);
    let result = Tracker::fail_after(0, || {
        provided::Arena::<u8, ()>::try_with(NonZero::<u32>::MIN, || {
            called.set(true);
            Ok::<(), alloc::Error>(())
        })
    });
    assert!(result.is_err());
    assert!(!called.get());
}

#[test]
fn context_failure_frees() {
    let (result, stats) = Tracker::activity(|| {
        hint::black_box(provided::Arena::<u8, ()>::try_with(
            NonZero::<u32>::MIN,
            || hint::black_box(Err::<(), _>(alloc::Error::Overflow)),
        ))
    });
    assert!(matches!(result, Err(alloc::Error::Overflow)));
    assert_eq!(stats.allocations, 1);
    assert_eq!(stats.deallocations, 1);
    assert_eq!(stats.allocated_bytes, stats.deallocated_bytes);
}

#[test]
fn reuses_rejected_slot() {
    let drops = Cell::new(Some(0));
    let arena =
        provided::Arena::<Item, _>::try_with(NonZero::<u32>::MIN, || Ok::<_, alloc::Error>(7_u8));
    let mut arena = arena.expect("the single slot must allocate");
    let buffer = Item::try_new(b"body", &drops).expect("fixture buffer must allocate");
    let pointer = buffer.bytes.as_ptr();
    let measured = Tracker::begin();
    arena.scope(Pin::new(&mut ()), |_, arena, _| {
        assert_eq!(*arena.context(), 7);
        assert_eq!(arena.capacity(), NonZero::<u32>::MIN);
        assert!(arena.vacant().is_some(), "the initial slot must be vacant");
        let vacant = arena
            .vacant()
            .expect("an unused vacancy must return its slot");
        let (rejected, stats) = Tracker::activity(|| vacant.try_insert(buffer, 2, Item::bytes_mut));
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.deallocations, 0);
        let rejected = rejected.expect_err("four bytes exceed the two-byte ceiling");
        let (buffer, error) = rejected.into_parts();
        assert_eq!(error.projected(), 4);
        assert_eq!(error.resident(), 2);
        assert_eq!(buffer.bytes.as_ptr(), pointer);
        assert_eq!(drops.get(), Some(0));

        {
            let vacant = arena
                .vacant()
                .expect("rejection must return its exact slot");
            let (owner, stats) =
                Tracker::activity(|| vacant.try_insert(buffer, 4, Item::bytes_mut));
            assert_eq!(stats.allocations, 0);
            assert_eq!(stats.deallocations, 0);
            let owner = owner.expect("buffer transition accepted");
            assert_eq!(owner.as_slice(), b"body");
            assert_eq!(owner.as_slice().as_ptr(), pointer);
        }
        assert!(arena.has_returned());
        {
            let returned = arena.pop_returned();
            assert!(matches!(&returned, Some(item) if item.bytes.as_ptr() == pointer));
        }
        assert_eq!(drops.get(), Some(1));
        assert!(
            arena.vacant().is_some(),
            "removal recycles the complete slot"
        );
    });
    assert_eq!(measured.finish().allocations, 0);
}
