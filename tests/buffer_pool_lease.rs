pub mod common;

use std::num::NonZero;

use common::{
    alloc::system::{Activity, Tracker},
    buffer::pool::Fill as _,
};
use o3::buffer::pool::{self, owners, state};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn admission_is_atomic() {
    let mut lease = (pool::Span::try_new(8)).expect("allocate the bounded destination");
    let source = b"abc";
    let (admitted, activity) = Tracker::activity(|| lease.try_append(source));
    assert!(activity == Activity::default());
    let append = (admitted).expect("admit the complete source");
    let (lease, activity) = Tracker::activity(|| append.commit());
    assert!(activity == Activity::default());
    assert_eq!(lease.as_slice(), source);
    let address = lease.as_slice().as_ptr();
    let result = lease.try_append(b"123456");
    assert!(result.is_err(), "reject six bytes with only five remaining");
    let Err(error) = result else { return };
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(5), 5)
    );
    assert_eq!(lease.as_slice(), source);
    {
        let pending = (lease.try_append(b"12345")).expect("admit the exact suffix");
        assert_eq!(pending.as_slice(), source);
    }
    assert_eq!(lease.as_slice(), source);
    lease
        .try_append(b"12345")
        .map(|append| append.commit())
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(lease.as_slice(), b"abc12345");
    assert_eq!(lease.as_slice().as_ptr(), address);
    lease
        .try_append(b"")
        .map(|append| append.commit())
        .as_ref()
        .expect("required fixture outcome");
    lease.truncate(3);
    lease
        .try_append(b"!")
        .map(|append| append.commit())
        .as_ref()
        .expect("required fixture outcome");
    lease.as_mut_slice().make_ascii_uppercase();
    assert_eq!(lease.as_slice(), b"ABC!");
}

#[test]
fn retains_prefix_sources() {
    let mut lease = (pool::Span::try_new(8)).expect("allocate a prefix destination");
    let first = b"ab".as_slice();
    let second = b"cdef".as_slice();
    let (admitted, activity) =
        Tracker::activity(|| pool::Append::try_prefixed(&mut lease, *b"<>!", [first, second]));
    assert!(activity == Activity::default());
    let result = admitted;
    assert!(result.is_err(), "reject the complete nine-byte operation");
    let Err(error) = result else { return };
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(8), 8)
    );
    assert_eq!(lease.len(), 0);
    let append = (pool::Append::try_prefixed(&mut lease, *b"<>", [first, second]))
        .expect("admit prefix and both immutable sources");
    let ((lease, sources), activity) = Tracker::activity(|| append.commit());
    assert!(activity == Activity::default());
    assert_eq!(sources[0].as_ptr(), first.as_ptr());
    assert_eq!(sources[1].as_ptr(), second.as_ptr());
    assert_eq!(lease.as_slice(), b"<>abcdef");
}

#[test]
fn copies_fixed_sources() {
    let layout = pool::Width::<16>::new(1).expect("one fixed-width slot");
    let pool = pool::Pool::<state::Initialized, owners::Plain<_>>::try_from(layout)
        .expect("allocate the fixed-width pool");
    let (lease, activity) = Tracker::activity(|| {
        let lease = pool
            .copy([b"abcd", b"efgh", b"ijkl", b"mnop"])
            .expect("the fixed-width slot is available");
        assert_eq!(lease.as_slice(), b"abcdefghijklmnop");
        let pointer = lease.as_slice().as_ptr();
        assert!(pool.copy([b"next"; 4]).is_none());
        drop(lease);
        let reused = pool
            .copy([b"next"; 4])
            .expect("the fixed-width slot was returned");
        assert_eq!(reused.as_slice().as_ptr(), pointer);
        reused
    });
    assert!(activity == Activity::default());
    let ((), activity) = Tracker::activity(|| drop(pool));
    assert!(activity == Activity::default());
    assert_eq!(lease.as_slice(), b"nextnextnextnext");
}

#[test]
fn retains_exact_output() {
    let pool =
        (pool::Pool::<state::Initialized>::try_new(1, 8)).expect("allocate initialized output");
    let mut lease = (pool.acquire()).expect("acquire the initialized slot");
    lease
        .output()
        .try_fill::<4>(|bytes| bytes.copy_from_slice(b"comp"))
        .expect("fixture write fits")
        .commit();
    let done = lease
        .output()
        .try_fill::<4>(|bytes| bytes.copy_from_slice(b"lete"))
        .expect("fixture write fits");
    let (lease, activity) = Tracker::activity(|| done.commit());
    assert!(activity == Activity::default());
    assert_eq!(lease.as_slice(), b"complete");
    lease.as_mut_slice().make_ascii_uppercase();
    assert_eq!(lease.as_slice(), b"COMPLETE");
    assert_eq!(lease.output().len(), 0);
}

#[test]
fn lease_outlives_pool() {
    let mut lease = {
        let pool = (pool::Pool::<state::Initialized>::try_new(1, 8))
            .expect("the initialized detached-lease pool must allocate");

        (pool.acquire()).expect("acquire the initialized slot")
    };
    lease
        .output()
        .try_fill::<4>(|bytes| bytes.copy_from_slice(b"body"))
        .expect("fixture write fits")
        .commit();
    assert_eq!(lease.as_slice(), b"body");
}

#[test]
fn keeps_initialized_bytes() {
    assert_eq!(
        std::mem::size_of::<pool::Pool<state::Initialized>>(),
        std::mem::size_of::<pool::Pool>(),
    );
    let pool = (pool::Pool::<state::Initialized>::try_new(1, 8))
        .expect("the initialized-byte reuse pool must allocate");
    {
        let mut lease = (pool.acquire()).expect("acquire the initialized slot");
        assert_eq!(lease.output().as_mut_slice(), &[0; 8]);
        lease
            .output()
            .try_fill::<4>(|bytes| {
                bytes.copy_from_slice(b"body");
                bytes.make_ascii_uppercase();
            })
            .expect("fixture write fits")
            .commit();
        assert_eq!(lease.as_slice(), b"BODY");
    }

    let mut reused = (pool.acquire()).expect("reacquire the initialized slot");
    assert_eq!(&reused.output().as_mut_slice()[..4], b"BODY");
    reused
        .output()
        .try_fill::<3>(|bytes| bytes.copy_from_slice(b"new"))
        .expect("fixture write fits")
        .commit();
    let frozen = reused.freeze();
    assert_eq!(frozen.as_ref(), b"new");
    assert!(pool.acquire().is_none());
    drop(frozen);
    assert!(pool.acquire().is_some());
}

fn release<S: state::State, L>(pool: pool::Pool<S, owners::Plain<L>>, bytes: usize) {
    let (slots, lease, closed) = {
        let pool = pool;
        let slots = pool.slots();
        let (lease, hot) = Tracker::activity(|| pool.acquire());
        assert!(hot == Activity::default());
        assert_eq!(lease.is_some(), slots != 0);
        (slots, lease, Tracker::begin())
    };
    let closed = closed.finish();
    let released = match lease {
        Some(lease) => {
            let released = {
                let lease = lease;
                assert!(closed == Activity::default());
                assert!(lease.is_empty());
                Tracker::begin()
            };
            released.finish()
        }
        None => {
            assert_eq!(slots, 0);
            closed
        }
    };
    assert_eq!(released.allocations, 0);
    assert_eq!(released.deallocations, 1);
    assert_eq!(released.deallocated_bytes, bytes);
}

#[test]
fn dynamic_geometry() {
    for (slots, capacity) in [(0, 1), (1, 1), (2, 3), (3, 17), (5, 127), (11, 4093)] {
        let layout = (pool::Layout::new(slots, capacity)).expect("valid pool geometry");
        let (created, build) = Tracker::activity(|| pool::Pool::<state::Uninit>::try_from(layout));
        let pool = (created).expect("pool allocation");
        assert_eq!(pool.slots(), slots);
        assert_eq!(pool.capacity().get() as usize, capacity);
        assert_eq!(pool.available(), slots);
        assert_eq!(build.allocations, 1);
        assert_eq!(build.deallocations, 0);
        release(pool, build.allocated_bytes);
    }
}

fn fixed<const N: usize, const C: usize>() {
    let layout = pool::Fixed::<N, C>::new();
    let (created, build) = Tracker::activity(|| {
        pool::Pool::<state::Initialized, owners::Plain<pool::Fixed<N, C>>>::try_from(layout)
    });
    let pool = (created).expect("fixed pool allocation");
    assert_eq!(build.allocations, 1);
    assert_eq!(build.deallocations, 0);
    release(pool, build.allocated_bytes);
}

#[test]
fn static_geometry() {
    fixed::<0, 1>();
    fixed::<1, 3>();
    fixed::<7, 17>();
    let layout = (pool::Width::<13>::new(5)).expect("valid static width");
    let (created, build) = Tracker::activity(|| {
        pool::Pool::<state::Uninit, owners::Plain<pool::Width<13>>>::try_from(layout)
    });
    let pool = (created).expect("width pool allocation");
    assert_eq!(build.allocations, 1);
    release(pool, build.allocated_bytes);
}

#[test]
fn rejects_before_alloc() {
    let (result, trace) = Tracker::activity(|| {
        pool::Pool::<state::Uninit>::try_new(u32::MAX as usize, u32::MAX as usize)
    });
    assert!(matches!(
        result,
        Err(pool::CreateError::Layout(
            pool::LayoutError::PlainDataArray {
                slots: u32::MAX,
                capacity: u32::MAX,
            }
        ))
    ));
    assert!(trace == Activity::default());
    let (result, trace) = Tracker::activity(|| pool::Pool::<state::Uninit>::try_new(1, 0));
    assert!(matches!(
        result,
        Err(pool::CreateError::Layout(pool::LayoutError::ZeroCapacity))
    ));
    assert!(trace == Activity::default());
    let (result, trace) = Tracker::activity(|| pool::Pool::<state::Uninit>::try_new(usize::MAX, 1));
    assert!(matches!(
        result,
        Err(pool::CreateError::Layout(pool::LayoutError::SlotsTooLarge))
    ));
    assert!(trace == Activity::default());
}

#[test]
fn exhaustion_keeps_failure() {
    let (failed, activity) = Tracker::activity(|| {
        Tracker::fail_after(0, || pool::Pool::<state::Uninit>::try_new(3, 17))
    });
    assert!(matches!(
        failed,
        Err(pool::CreateError::Exhausted(o3::alloc::Exhausted))
    ));
    assert!(activity == Activity::default());
    let (failed, activity) = Tracker::activity(|| {
        Tracker::fail_after(0, || {
            pool::Pool::<state::Initialized, owners::Plain<pool::Fixed<3, 17>>>::try_from(
                pool::Fixed::new(),
            )
        })
    });
    assert!(matches!(failed, Err(o3::alloc::Exhausted)));
    assert!(activity == Activity::default());
}
