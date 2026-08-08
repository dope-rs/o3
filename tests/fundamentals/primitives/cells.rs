use o3::{
    buffer::{
        BLOCK_CAPACITY, PrefixConsumer,
        pool::{Cursor, FixedCapacity, Layout, LayoutError, Pool, state::Uninitialized},
        storage::shared::{Shared, strings::Str},
    },
    cell::branded::{Brand, BrandToken, Region},
    mem::{
        budget::Bytes,
        fair::{Credits, State},
    },
};

type FixedPool = Pool<Uninitialized, FixedCapacity<BLOCK_CAPACITY>>;
type FixedLease = Cursor<FixedCapacity<BLOCK_CAPACITY>>;
const FIXED_CAPACITY: usize = BLOCK_CAPACITY as usize;

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
    let pool = o3::mem::fair::Pool::new([0, 9]);
    let first = State::split_at([3, 21], 2, 0);
    let second = State::split_at([3, 21], 2, 1);
    let first_credit = pool.bind(&first);
    let second_credit = pool.bind(&second);

    assert!(second_credit.try_acquire_all([1, 10]));
    assert!(!second_credit.try_acquire_all([1, 1]));
    assert!(first_credit.try_acquire_all([2, 11]));
    first_credit.release_all([2, 11]);
    assert!(first_credit.try_acquire_all([2, 20]));
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
    BrandToken::scope(|mut brand| {
        let value = Brand::new(1);
        *value.borrow_mut(&mut brand) = 2;
        assert_eq!(*value.borrow(&brand), 2);
    });
}

#[test]
fn application_and_state_permissions_are_independent() {
    BrandToken::scope_with_region(|mut app, mut state| {
        let dispatcher = Brand::new(1);
        let storage = Region::new(2);

        let dispatcher = dispatcher.borrow_mut(&mut app);
        *storage.borrow_mut(&mut state) += *dispatcher;

        assert_eq!(*storage.borrow(&state), 3);
    });
}
