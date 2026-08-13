use std::{marker::PhantomPinned, ptr};

use o3::{
    buffer::{
        BLOCK_CAPACITY, Layout, Pool, PrefixConsumer,
        pool::{Cursor, FixedCapacity, LayoutError, state::Uninitialized},
        storage::{Shared, strings::Str},
    },
    cell::{LocalRefCount, StableLink, brand, region},
    mem::{
        budget::Bytes,
        credit::Ledger,
        fair::{self, Credits},
    },
};

struct StableValue {
    value: u32,
    _pin: PhantomPinned,
}

mod sealed;

type FixedPool = Pool<Uninitialized, FixedCapacity<BLOCK_CAPACITY>>;
type FixedLease = Cursor<FixedCapacity<BLOCK_CAPACITY>>;
const FIXED_CAPACITY: usize = BLOCK_CAPACITY as usize;

#[test]
fn stable_links_are_one_word_and_borrow_their_pinned_target() {
    let value = Box::pin(StableValue {
        value: 7,
        _pin: PhantomPinned,
    });
    let link = StableLink::from_stable(sealed::StableSource(value.as_ref()));
    let copy = link;

    assert_eq!(link.get().value, 7);
    assert_eq!(copy.get().value, 7);
    assert!(link == copy);
    assert_eq!(
        size_of::<StableLink<StableValue>>(),
        size_of::<ptr::NonNull<StableValue>>()
    );
}

#[test]
fn local_ref_counts_separate_strict_and_tolerant_transitions() {
    let refs = LocalRefCount::empty();
    assert_eq!(size_of::<LocalRefCount>(), size_of::<u32>());
    assert!(refs.is_empty());
    assert!(!refs.try_retain());
    assert_eq!(refs.try_release(), None);

    assert!(refs.try_activate());
    assert!(!refs.try_activate());
    assert!(refs.is_unique());
    assert!(refs.try_retain());
    assert!(!refs.is_unique());
    assert_eq!(refs.try_release(), Some(false));
    assert!(refs.is_unique());

    assert_eq!(refs.try_release(), Some(true));
    assert!(refs.is_unique());
    assert!(refs.try_deactivate());
    assert!(refs.is_empty());
    assert!(!refs.try_deactivate());

    let one = LocalRefCount::one();
    one.retain();
    assert!(!one.release());
    assert!(one.release());
    one.deactivate();
    one.activate();
    assert!(one.is_unique());
}

#[test]
fn fixed_pool_capacity_adds_no_runtime_state() {
    if usize::BITS == 64 {
        assert_eq!(size_of::<Pool>(), 8);
        assert_eq!(size_of::<FixedPool>(), 8);
        assert_eq!(size_of::<Cursor>(), 24);
        assert_eq!(size_of::<FixedLease>(), 24);
    }
}

#[test]
fn pooled_buffers_enforce_capacity_and_recycle_leases() {
    let pool = FixedPool::fixed::<1>();
    let mut buffer = pool.try_acquire_buffer().unwrap();
    let block = vec![b'x'; FIXED_CAPACITY];
    buffer.try_extend(&block).unwrap();
    let overflow = buffer.try_push(b'e').unwrap_err();
    assert_eq!(buffer.as_ref(), block);
    assert_eq!(
        overflow.to_string(),
        format!(
            "capacity exceeded: attempted {}, capacity {}",
            FIXED_CAPACITY + 1,
            FIXED_CAPACITY
        )
    );
    assert!(pool.try_acquire().is_none());
    drop(buffer);
    assert_eq!(pool.available(), 1);
}

#[test]
fn shared_str_validates_utf8_without_copying() {
    let shared = Shared::from(String::from("hello"));
    let ptr = shared.as_ptr();
    let text = Str::from_utf8(shared).unwrap();
    let clone = text.clone();
    assert_eq!(text.as_str(), "hello");
    assert_eq!(clone.as_bytes(), b"hello");
    assert_eq!(text.as_bytes().as_ptr(), ptr);
    assert_eq!(clone.as_bytes().as_ptr(), ptr);
    assert!(Str::from_utf8(Shared::from(vec![0xff])).is_err());
}

#[test]
fn byte_budget_returns_capacity_when_a_lease_drops() {
    let budget = Bytes::new(4);
    let handle = budget.handle();
    let lease = handle.try_acquire(3).unwrap();
    assert!(handle.try_acquire(2).is_none());
    drop(lease);
    assert!(handle.try_acquire(4).is_some());
}

#[test]
fn credit_ledger_acquires_and_releases_without_allocation() {
    let ledger = Ledger::new(5);
    assert_eq!(ledger.limit(), 5);
    assert_eq!(ledger.available(), 5);
    assert!(ledger.try_acquire(3));
    assert_eq!(ledger.used(), 3);
    assert_eq!(ledger.available(), 2);
    assert!(!ledger.try_acquire(usize::MAX));
    assert_eq!(ledger.used(), 3);
    ledger.release(2);
    assert_eq!(ledger.used(), 1);
    assert!(ledger.try_acquire(4));
    ledger.release(5);
    assert_eq!(ledger.available(), 5);
}

#[test]
fn fair_credits_protect_each_lane_and_share_the_rest() {
    let credits = Credits::with_reserve(8, 2, 2);
    assert!(credits.try_acquire(0, 6));
    assert!(!credits.try_acquire(0, 1));
    assert!(credits.try_acquire(1, 2));
    assert_eq!(credits.used(), 8);

    credits.release(0, 3);
    assert_eq!(credits.shared_available(), 3);
    assert!(credits.try_acquire(1, 1));
    assert_eq!(credits.held_by(0), Some(3));
    assert_eq!(credits.held_by(1), Some(3));
    assert_eq!(credits.reserved_for(0), Some(2));
    assert_eq!(credits.reserved_for(1), Some(2));

    let used = credits.used();
    assert!(!credits.try_acquire(0, 9));
    assert_eq!(credits.used(), used);
}

#[test]
fn fair_credits_acquire_multiple_dimensions_atomically() {
    let credits = Credits::from_capacities([8, 80], 2);

    assert!(!credits.try_acquire_all(0, [6, 61]));
    assert!(credits.try_acquire_all(0, [6, 60]));
    assert!(credits.try_acquire_all(1, [2, 20]));

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        credits.release_all(0, [3, 61]);
    }));
    assert!(caught.is_err());
    credits.release_all(0, [6, 60]);
    credits.release_all(1, [2, 20]);
    assert!(credits.try_acquire_all(0, [6, 60]));
    assert!(credits.try_acquire_all(1, [2, 20]));
}

#[test]
fn fair_credits_split_exact_total_reserves_without_stranding_remainders() {
    let pool = fair::Pool::split([0, 9], [3, 21], 2);
    let first_credit = pool.lane(0).unwrap();
    let second_credit = pool.lane(1).unwrap();

    assert!(second_credit.try_acquire_all([1, 10]));
    assert!(!second_credit.try_acquire_all([1, 1]));
    assert!(first_credit.try_acquire_all([2, 11]));
    first_credit.release_all([2, 11]);
    assert!(first_credit.try_acquire_all([2, 20]));
}

#[test]
fn fair_pool_owns_each_lanes_accounting_identity() {
    let first = fair::Pool::split([10], [0], 1);
    let second = fair::Pool::split([10], [0], 1);
    let first_lane = first.lane(0).unwrap();
    let second_lane = second.lane(0).unwrap();

    assert!(first_lane.try_acquire_all([5]));
    let cross_release = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        second_lane.release_all([5]);
    }));
    assert!(cross_release.is_err());

    first_lane.release_all([5]);
    assert!(first_lane.try_acquire_all([10]));
    assert!(second_lane.try_acquire_all([10]));
}

#[test]
fn pooled_buffers_extend_from_slices_with_one_reservation() {
    let pool = FixedPool::fixed::<1>();
    let mut lease = pool.try_acquire_buffer().unwrap();
    lease
        .try_extend_from_slices([&b"ab"[..], &b"cde"[..]])
        .unwrap();
    assert_eq!(lease.as_ref(), b"abcde");
    let overflow = vec![b'x'; FIXED_CAPACITY - 4];
    assert!(
        lease
            .try_extend_from_slices([overflow.as_slice(), &[]])
            .is_err()
    );
    assert_eq!(lease.as_ref(), b"abcde");
}

#[test]
fn pooled_buffers_reuse_consumed_prefixes() {
    let pool = FixedPool::fixed::<1>();
    let mut lease = pool.try_acquire_buffer().unwrap();
    lease.try_extend(b"abcd").unwrap();
    lease.try_consume_prefix(3).unwrap().commit();
    lease.try_extend(b"efgh").unwrap();
    assert_eq!(lease.as_ref(), b"defgh");
    let fill = vec![b'x'; FIXED_CAPACITY - lease.len()];
    lease.try_extend(&fill).unwrap();
    assert!(lease.try_push(b'i').is_err());
    assert_eq!(&lease.as_ref()[..5], b"defgh");
}

#[test]
fn runtime_pool_uses_its_configured_slot_capacity() {
    let layout = Layout::new(2, 31).expect("the test pool layout is valid");
    let pool = Pool::from_layout(layout);
    let mut lease = pool
        .try_acquire_buffer()
        .expect("the configured pool has two free slots");
    lease
        .try_extend(&[b'x'; 31])
        .expect("one configured slot must fit exactly");
    assert!(lease.try_push(b'y').is_err());
}

#[test]
fn runtime_pool_layout_rejects_only_invalid_allocation_shapes() {
    assert!(matches!(Layout::new(1, 0), Err(LayoutError::ZeroCapacity)));
    assert!(matches!(
        Layout::new(u32::MAX as usize, u32::MAX as usize),
        Err(LayoutError::CapacityOverflow)
    ));

    let empty = Layout::new(0, 1).expect("a zero-slot pool has a valid empty layout");
    let pool: Pool = Pool::from_layout(empty);
    assert!(pool.try_acquire().is_none());
}

#[test]
fn local_pool_owners_are_movable() {
    fn assert_unpin<T: Unpin>() {}

    assert_unpin::<Pool>();
    assert_unpin::<FixedPool>();

    let pool = FixedPool::fixed::<1>();
    let lease = pool
        .try_acquire_buffer()
        .expect("movable pool has one slot");
    drop(lease);
    assert_eq!(pool.available(), 1);
}

#[test]
fn brand_cells_mutate_in_place() {
    brand::Token::scope(|mut token| {
        let value = brand::Value::new(1);
        *value.borrow_mut(&mut token) = 2;
        assert_eq!(*value.borrow(&token), 2);
    });
}

#[test]
fn application_and_state_permissions_are_independent() {
    brand::Token::scope_with_region(|mut app, mut state| {
        let dispatcher = brand::Value::new(1);
        let storage = region::Value::new(2);

        let dispatcher = dispatcher.borrow_mut(&mut app);
        *storage.borrow_mut(&mut state) += *dispatcher;

        assert_eq!(*storage.borrow(&state), 3);
    });
}
