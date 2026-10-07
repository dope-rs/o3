pub mod common;

use std::{array, num::NonZero};

use common::{
    alloc::system::{Activity, Tracker},
    buffer::pool::Reentrant,
};
use o3::{
    alloc,
    buffer::{
        self, BLOCK_CAPACITY,
        pool::{self, Fixed, Growing, Layout, Pool, owners, state::Uninit},
        write::{Sink as _, consume::Consume},
    },
};

#[global_allocator]
static ALLOCATOR: Reentrant = Reentrant;

const FIXED_CAPACITY: usize = BLOCK_CAPACITY as usize;
type FixedPool = Pool<Uninit, owners::Plain<Fixed<1, FIXED_CAPACITY>>>;

#[test]
fn recycles_pool_leases() {
    let created = FixedPool::try_from(Fixed::new());
    let pool: FixedPool = created.expect("fixed recycle pool must allocate one full block");
    {
        let mut buffer = (pool.cursor()).expect("fixed pool has one free slot");
        let block = vec![b'x'; FIXED_CAPACITY];
        buffer
            .write_slice(&block)
            .as_ref()
            .expect("required fixture outcome");
        let pushed = buffer.write_byte(b'e');
        assert!(
            pushed.is_err(),
            "full fixed buffer must reject another byte"
        );
        let Err(error) = pushed else {
            return;
        };
        assert_eq!(buffer.as_ref(), block);
        assert_eq!(
            {
                let (required, available) = error.into_parts();
                (required.get(), available)
            },
            (1, 0)
        );
        assert!(pool.acquire().is_none());
        buffer.recycle();
    }
    assert_eq!(pool.available(), 1);
}

#[test]
fn extends_from_slices() {
    let created = FixedPool::try_from(Fixed::new());
    let pool: FixedPool = created.expect("slice-write pool must allocate one full block");
    let mut lease = (pool.cursor()).expect("fixed pool has one free slot");
    lease
        .write_slices([&b"ab"[..], &b"cde"[..]])
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(lease.as_ref(), b"abcde");
    let extra = vec![b'x'; FIXED_CAPACITY - 4];
    assert!(lease.write_slices([extra.as_slice(), &[]]).is_err());
    assert_eq!(lease.as_ref(), b"abcde");
}

#[test]
fn reuses_consumed_prefix() {
    let created = FixedPool::try_from(Fixed::new());
    let pool: FixedPool = created.expect("prefix-reuse pool must allocate one full block");
    let mut lease = (pool.cursor()).expect("fixed pool has one free slot");
    lease
        .write_slice(b"abcd")
        .as_ref()
        .expect("required fixture outcome");
    let consumed = lease.try_consume(3);
    consumed
        .as_ref()
        .expect("initialized three-byte prefix must be consumable");
    lease
        .write_slice(b"efgh")
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(lease.as_ref(), b"defgh");
    let fill = vec![b'x'; FIXED_CAPACITY - lease.len()];
    lease
        .write_slice(&fill)
        .as_ref()
        .expect("required fixture outcome");
    assert!(lease.write_byte(b'i').is_err());
    assert_eq!(&lease.as_ref()[..5], b"defgh");
}

#[test]
fn uses_runtime_capacity() {
    let laid_out = Layout::new(2, 31);
    let layout = laid_out.expect("two runtime slots of 31 bytes must be representable");
    let created = Pool::try_from(layout);
    let pool = created.expect("runtime pool storage must allocate");
    let mut lease = (pool.cursor()).expect("the configured pool has two free slots");
    {
        (lease.write_slice(&[b'x'; 31]))
            .as_ref()
            .expect("required fixture outcome");
    };
    assert!(lease.write_byte(b'y').is_err());
}

#[test]
fn moves_pool_owners() {
    fn assert_unpin<T: Unpin>() {}

    assert_unpin::<Pool>();
}

#[test]
fn zero_slots_stays_empty() {
    let result = Layout::new(0, 8);
    let plan = result.expect("eight-byte slots must admit an empty pool");
    assert_eq!(plan.slots(), 0);
    let (result, activity) = Tracker::activity(|| Growing::try_new(plan));
    assert_eq!(activity, Activity::default());
    let mut pool = result.expect("an empty pool must require no slot group");
    let result = pool.try_acquire();
    let lease = result.expect("an empty pool acquisition must require no allocation");
    assert!(lease.is_none());
    let ((), activity) = Tracker::activity(|| {
        assert_eq!(pool.capacity().get(), 8);
        assert_eq!(pool.available(), 0);
        assert!(pool.try_borrow().expect("empty borrowed pool").is_none());
        assert!(pool.try_view().expect("empty cursor pool").is_none());
    });
    assert_eq!(activity, common::alloc::system::Activity::default());
}

#[test]
fn grows_to_plan_limit() {
    let result = Layout::new(3, 8);
    let plan = result.expect("three eight-byte slots must form a valid pool");
    assert_eq!(plan.slots(), 3);
    let result = Growing::try_new(plan);
    let mut pool = result.expect("the first slot group must allocate");
    let mut leases = Vec::new();
    for _ in 0..3 {
        let result = pool.try_acquire();
        let lease = result.expect("each acquisition within the plan must grow successfully");
        leases.push(lease.expect("each planned slot must be available"));
    }
    let result = pool.try_acquire();
    let lease = result.expect("an exhausted planned pool must require no allocation");
    assert!(lease.is_none());

    assert!(leases.pop().is_some(), "three planned slots must be held");
    let result = pool.try_acquire();
    let lease = result.expect("a returned slot must be reusable without allocation");
    assert!(lease.is_some(), "the returned slot must become available");
}

#[test]
fn leases_survive_pool_drop() {
    let total = Tracker::begin();
    let mut pool = Growing::try_new(Layout::constant::<64, 8>()).expect("bounded pool");
    let mut leases: [pool::Span; 64] =
        array::from_fn(|_| pool.try_acquire().expect("allocation").expect("slot"));
    let owner = Tracker::begin();
    drop(pool);
    let owner = owner.finish();
    assert_eq!((owner.allocations, owner.deallocations), (0, 1));
    assert_eq!(owner.deallocated_bytes, 7 * std::mem::size_of::<Pool>());
    leases[0]
        .try_append(b"lifetime")
        .expect("retained slot")
        .commit();
    assert_eq!(leases[0].as_slice(), b"lifetime");
    drop(leases);
    let total = total.finish();
    assert_eq!((total.allocations, total.deallocations), (8, 8));
    assert_eq!(total.allocated_bytes, total.deallocated_bytes);
}

#[test]
fn commits_exact_source() {
    let pool = pool::Pool::try_new(1, 8).expect("copy pool");
    let ((), activity) = Tracker::usage(|| {
        {
            let mut target = pool.acquire().expect("free copy slot");
            let prefix = target.try_append(b"ab").expect("initial prefix");
            prefix.commit();
            let pointer = target.as_slice().as_ptr();
            let source = buffer::raw::Slices::try_new([b"cd".as_slice(), b"", b"efgh"])
                .expect("bounded copy source")
                .expect("nonempty copy source");
            assert_eq!(source.len().get(), 6);
            let admitted = pool::Copy::try_new(source, target).expect("complete copy fits");
            let owner = admitted.commit();
            assert_eq!(owner.as_ref(), b"abcdefgh");
            assert_eq!(owner.as_ref().as_ptr(), pointer);
            assert_eq!(pool.available(), 0);
        }
        assert_eq!(pool.available(), 1);
    });
    assert_eq!(activity, (0, 0));
}

#[test]
fn rejects_short_tail() {
    let pool = pool::Pool::try_new(1, 8).expect("copy pool");
    let mut target = pool.acquire().expect("free copy slot");
    let prefix = target.try_append(b"ab").expect("initial prefix");
    prefix.commit();
    let source = buffer::raw::Slices::try_new([b"1234567".as_slice()])
        .expect("bounded copy source")
        .expect("nonempty copy source");
    let (admitted, activity) = Tracker::usage(|| pool::Copy::try_new(source, target));
    let result = admitted;
    assert!(result.is_err(), "copy exceeds remaining capacity");
    let Err(error) = result else { return };
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (7, 6));
    assert_eq!(activity, (0, 0));
    assert_eq!(pool.available(), 1);
}

#[test]
fn drop_releases_admission() {
    let pool = pool::Pool::try_new(1, 8).expect("copy pool");
    let target = pool.acquire().expect("free copy slot");
    let source = buffer::raw::Slices::try_new([b"bytes".as_slice()])
        .expect("bounded copy source")
        .expect("nonempty copy source");
    assert!(
        pool::Copy::try_new(source, target).is_ok(),
        "complete copy fits"
    );
    assert_eq!(pool.available(), 1);
}

#[test]
fn bounds_sources_once() {
    assert!(matches!(
        buffer::raw::Slices::try_new([b"".as_slice(); 3]),
        Ok(None)
    ));
    let chunk = vec![0; 1 << 20];
    let source = buffer::raw::Slices::try_new([chunk.as_slice(); 4096]);
    let result = source;
    assert!(result.is_err(), "source exceeds representable capacity");
    let Err(error) = result else { return };
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (1 << 20, (1 << 20) - 1));
}

#[test]
fn preserves_alloc_failure() {
    let source = buffer::raw::Slices::try_new([b"ab".as_slice(), b"cd"])
        .expect("bounded copy source")
        .expect("nonempty copy source");
    let result = Tracker::fail_after(0, || source.try_owned());
    assert!(matches!(result, Err(o3::alloc::Exhausted)));
    let source = buffer::raw::Slices::try_new([b"ab".as_slice(), b"cd"])
        .expect("bounded copy source")
        .expect("nonempty copy source");
    let (result, activity) = Tracker::usage(|| source.try_owned());
    let owned = result.expect("source allocation");
    assert_eq!(owned.as_slice(), b"abcd");
    assert_eq!(activity.0, 1);
}

#[test]
fn rolls_back_growth() {
    for (slots, groups) in [(1, 1), (4, 3)] {
        let plan = pool::Layout::new(slots, 32).expect("bounded growing-pool plan");
        let (create, activity) =
            Tracker::activity(|| Tracker::fail_after(1, || pool::Growing::try_new(plan)));
        assert!(matches!(create, Err(alloc::Exhausted)));
        assert_eq!(activity.allocations, 1);
        assert_eq!(activity.deallocations, 1);
        assert_eq!(
            activity.allocated_bytes,
            groups * std::mem::size_of::<pool::Pool>()
        );
        assert_eq!(activity.allocated_bytes, activity.deallocated_bytes);
    }

    let plan = (pool::Layout::new(4, 32)).expect("the four-slot growing-pool plan must be valid");
    let mut growing = (pool::Growing::try_new(plan))
        .expect("the growth rollback fixture must allocate initially");
    let observed = growing.try_acquire();
    assert!(
        matches!(observed, Ok(Some(_))),
        "the initial growing-pool slot must be available without growth"
    );
    let Ok(Some(lease)) = observed else { return };
    let (growth, trace) = Tracker::activity(|| Tracker::fail_after(0, || growing.try_acquire()));
    assert!(matches!(growth, Err(alloc::Exhausted)));
    assert!(trace == Activity::default());

    assert_eq!(lease.capacity().get(), 32);
    {
        let released = lease;
        assert_eq!(released.len(), 0);
    }
    let (reused, trace) = Tracker::activity(|| growing.try_acquire());
    let reused = (reused).expect("reusing the released slot must not allocate");
    assert!(reused.is_some());
    assert!(trace == Activity::default());
}

#[test]
fn rechecks_reentry() {
    use common::buffer::pool::REENTRANT;

    REENTRANT.with_borrow_mut(|owner| {
        let (growing, observed) =
            Reentrant::on_alloc(|| pool::Growing::try_new(pool::Layout::constant::<2, 8>()));
        assert!(matches!(
            observed,
            Err(common::buffer::pool::Error::Borrow(_))
        ));
        *owner = Some(growing.expect("pool"));
    });
    REENTRANT.with_borrow(|owner| {
        let growing = owner.as_ref().expect("pool");
        let first = growing
            .try_borrow()
            .expect("allocation")
            .expect("warm slot");
        let ((second, observed), activity) =
            Tracker::activity(|| Reentrant::on_alloc(|| growing.try_borrow()));
        observed.expect("shared observer");
        let second = second.expect("allocation").expect("reentrant group");
        assert_eq!((activity.allocations, activity.deallocations), (2, 1));
        assert_eq!(growing.available(), 0);
        drop((first, second));
        assert_eq!(growing.available(), 2);
    });
    REENTRANT.with_borrow_mut(|owner| drop(owner.take()));
}

#[test]
fn borrows_across_growth() {
    let plan = pool::Layout::new(5, 8).expect("five bounded slots");
    let warm = NonZero::new(2).expect("two warm slots");
    let growing = pool::Growing::try_warm(plan, warm).expect("warm pool");
    assert_eq!(growing.available(), 5);
    let warm = Tracker::begin();
    let mut cursor = growing.try_view().expect("first warm slot").expect("slot");
    let mut spare = cursor.spare();
    assert_eq!(spare.write(b"body"), 4);
    spare.commit();
    let first = cursor.freeze();
    let pointer = first.as_slice().as_ptr();
    let clone = first.try_clone().expect("retained warm slot");
    let second = growing
        .try_borrow()
        .expect("second warm slot")
        .expect("slot");
    assert_eq!(warm.finish(), Activity::default());
    let (failed, activity) = Tracker::activity(|| Tracker::fail_after(0, || growing.try_view()));
    assert!(matches!(failed, Err(alloc::Exhausted)));
    assert_eq!(activity, Activity::default());
    assert_eq!(growing.available(), 3);
    let third = growing.try_view().expect("first growth").expect("slot");
    let fourth = growing.try_borrow().expect("grown slot").expect("slot");
    let fifth = growing
        .try_view()
        .expect("bounded final growth")
        .expect("slot");
    assert_eq!(growing.available(), 0);
    assert!(growing.try_borrow().expect("logical exhaustion").is_none());
    assert_eq!(first.as_slice(), b"body");
    assert_eq!(first.as_slice().as_ptr(), pointer);
    drop(first);
    assert_eq!(growing.available(), 0);
    assert_eq!(clone.as_slice(), b"body");
    drop(clone);
    let ((), activity) = Tracker::activity(|| {
        let cursor = growing
            .try_view()
            .expect("released warm slot")
            .expect("slot");
        assert_eq!(cursor.as_ref().as_ptr(), pointer);
        assert_eq!(growing.available(), 0);
    });
    assert_eq!(activity, Activity::default());
    drop((second, third, fourth, fifth));
    assert_eq!(growing.available(), 5);
}

#[test]
fn rejects_warm() {
    for (maximum, request) in [(0, 1), (2, 3)] {
        let plan = pool::Layout::new(maximum, 8).expect("bounded warm plan");
        let request = NonZero::new(request).expect("nonzero request");
        let (result, activity) = Tracker::activity(|| pool::Growing::try_warm(plan, request));
        assert!(matches!(
            result,
            Err(pool::GrowingError::Warm { requested: actual, maximum: limit })
                if actual == request && limit as usize == maximum
        ));
        assert_eq!(activity, Activity::default());
    }

    let plan = pool::Layout::new(2, 8).expect("bounded warm plan");
    let warm = NonZero::new(2).expect("two warm slots");
    for allowed in 0..2 {
        let (result, activity) = Tracker::activity(|| {
            Tracker::fail_after(allowed, || pool::Growing::try_warm(plan, warm))
        });
        assert!(matches!(
            result,
            Err(pool::GrowingError::Exhausted(alloc::Exhausted))
        ));
        assert_eq!(activity.allocations, allowed);
        assert_eq!(activity.deallocations, allowed);
        assert_eq!(activity.allocated_bytes, activity.deallocated_bytes);
    }
}
