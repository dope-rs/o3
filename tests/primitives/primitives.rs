use std::pin::pin;

use o3::buffer::{BlockLease, BlockPool, Pool, PoolLayout, PoolLayoutError, Shared, SharedStr};
use o3::cell::{BrandCell, BrandToken, RegionCell};
use o3::mem::{ByteBudget, FairCredits, ScratchVec};

#[test]
fn pooled_buffers_enforce_capacity_and_recycle_leases() {
    assert_eq!(std::mem::size_of::<BlockLease<'static>>(), 32);
    let pool = pin!(BlockPool::new(1));
    let mut buffer = pool.as_ref().try_acquire().unwrap();
    let block = vec![b'x'; BlockPool::CAPACITY];
    buffer.try_extend_from_slice(&block).unwrap();
    let overflow = buffer.try_push(b'e').unwrap_err();
    assert_eq!(buffer.as_ref(), block);
    assert_eq!(overflow.attempted(), BlockPool::CAPACITY + 1);
    assert_eq!(overflow.capacity(), BlockPool::CAPACITY);
    assert!(pool.as_ref().try_acquire().is_none());
    drop(buffer);
    assert_eq!(pool.available(), 1);
}

#[test]
fn shared_str_validates_utf8_without_copying() {
    let shared = Shared::from(String::from("hello"));
    let ptr = shared.as_ptr();
    let text = SharedStr::from_utf8(shared).unwrap();
    let clone = text.clone();
    assert_eq!(text.as_str(), "hello");
    assert_eq!(clone.as_bytes(), b"hello");
    assert_eq!(text.as_bytes().as_ptr(), ptr);
    assert_eq!(clone.as_bytes().as_ptr(), ptr);
    assert!(SharedStr::from_utf8(Shared::from(vec![0xff])).is_err());
}

#[test]
fn byte_budget_returns_capacity() {
    let budget = pin!(ByteBudget::new(4));
    let handle = budget.as_ref().handle();
    let mut lease = handle.try_acquire(3).unwrap();
    assert!(handle.try_acquire(2).is_none());
    lease.shrink(1);
    assert_eq!(lease.amount(), 2);
    let remainder = handle.try_acquire(2).unwrap();
    drop(remainder);
    drop(lease);
    assert_eq!(handle.used(), 0);
}

#[test]
fn fair_credits_protect_each_lane_and_share_the_rest() {
    let mut credits = FairCredits::with_reserve(8, 2, 2);
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
    let mut credits = FairCredits::from_capacities([8, 80], 2);

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
fn pooled_buffers_extend_from_slices_with_one_reservation() {
    let pool = std::pin::pin!(BlockPool::new(1));
    let mut lease = pool.as_ref().try_acquire().unwrap();
    lease
        .try_extend_from_slices([&b"ab"[..], &b"cde"[..]])
        .unwrap();
    assert_eq!(lease.as_ref(), b"abcde");
    let overflow = vec![b'x'; BlockPool::CAPACITY - 4];
    assert!(
        lease
            .try_extend_from_slices([overflow.as_slice(), &[]])
            .is_err()
    );
    assert_eq!(lease.as_ref(), b"abcde");
}

#[test]
fn pooled_buffers_reuse_consumed_prefixes() {
    let pool = std::pin::pin!(BlockPool::new(1));
    let mut lease = pool.as_ref().try_acquire().unwrap();
    lease.spare_writer().try_extend_from_slice(b"abcd").unwrap();
    lease.consume(3);
    lease.try_extend_from_slice(b"efgh").unwrap();
    assert_eq!(lease.as_ref(), b"defgh");
    let fill = vec![b'x'; BlockPool::CAPACITY - lease.len()];
    lease.try_extend_from_slice(&fill).unwrap();
    assert!(lease.try_push(b'i').is_err());
    assert_eq!(&lease.as_ref()[..5], b"defgh");
}

#[test]
fn runtime_pool_uses_its_configured_slot_capacity() {
    let layout = PoolLayout::new(2, 31).expect("the test pool layout is valid");
    let pool = pin!(Pool::new(layout));
    let mut lease = pool
        .as_ref()
        .try_acquire()
        .expect("the configured pool has two free slots");
    assert_eq!(lease.capacity(), 31);
    lease
        .try_extend_from_slice(&[b'x'; 31])
        .expect("one configured slot must fit exactly");
    assert!(lease.try_push(b'y').is_err());
}

#[test]
fn runtime_pool_layout_rejects_only_invalid_allocation_shapes() {
    assert_eq!(PoolLayout::new(1, 0), Err(PoolLayoutError::ZeroCapacity));
    assert_eq!(
        PoolLayout::new(u32::MAX, u32::MAX),
        Err(PoolLayoutError::CapacityOverflow)
    );

    let empty = PoolLayout::new(0, 1).expect("a zero-slot pool has a valid empty layout");
    let pool = pin!(Pool::new(empty));
    assert!(pool.as_ref().try_acquire().is_none());
}

#[test]
fn brand_cells_mutate_in_place() {
    BrandToken::scope(|mut brand| {
        let value = BrandCell::new(1);
        *value.borrow_mut(&mut brand) = 2;
        assert_eq!(*value.borrow(&brand), 2);
    });
}

#[test]
fn application_and_state_permissions_are_independent() {
    BrandToken::scope_with_region(|mut app, mut state| {
        let dispatcher = BrandCell::new(1);
        let storage = RegionCell::new(2);

        let dispatcher = dispatcher.borrow_mut(&mut app);
        *storage.borrow_mut(&mut state) += *dispatcher;

        assert_eq!(*storage.borrow(&state), 3);
    });
}

#[test]
fn scratch_reuses_only_its_vector_storage() {
    let scratch = ScratchVec::new();
    let mut value = Vec::with_capacity(8);
    value.push(1u8);
    scratch.put(value);
    assert!(scratch.take().capacity() >= 8);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(std::mem::size_of::<ScratchVec<u8>>(), 24);
}
