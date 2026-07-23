use crate::confined::assert_confined;
use o3::buffer::{
    InitializedSharedLease, InitializedSharedPool, PoolLayoutError, Pooled, SharedLease, SharedPool,
};

assert_confined!(SharedPool);
assert_confined!(SharedLease);
assert_confined!(InitializedSharedPool);
assert_confined!(InitializedSharedLease);
assert_confined!(Pooled);

#[test]
fn frozen_slots_return_after_the_last_clone() {
    let pool = SharedPool::new(1, 16);
    let mut lease = pool.try_acquire().unwrap();
    lease.spare_writer().try_extend_from_slice(b"body").unwrap();
    let body = lease.freeze();
    let clone = body.clone();
    assert!(pool.try_acquire().is_none());
    drop(body);
    assert!(pool.try_acquire().is_none());
    drop(clone);
    assert!(pool.try_acquire().is_some());

    let empty = SharedPool::new(0, 16);
    assert_eq!(empty.capacity(), 16);
    assert_eq!(empty.available(), 0);
    assert!(empty.try_acquire().is_none());
}

#[test]
fn frozen_slot_outlives_the_pool_handle() {
    let body = {
        let pool = SharedPool::new(1, 8);
        let mut lease = pool.try_acquire().unwrap();
        assert!(lease.is_empty());
        lease.spare_writer().try_extend_from_slice(b"abc").unwrap();
        assert_eq!(lease.len(), 3);
        assert_eq!(lease.as_slice(), b"abc");
        lease.freeze()
    };
    assert_eq!(body.as_ref(), b"abc");
}

#[test]
fn invalid_layout_is_reported_before_allocation() {
    assert!(matches!(
        SharedPool::try_new(1, 0),
        Err(PoolLayoutError::ZeroCapacity)
    ));
    assert!(matches!(
        SharedPool::try_new(usize::MAX, 1),
        Err(PoolLayoutError::SlotOverflow)
    ));
    assert!(matches!(
        SharedPool::try_new(u32::MAX as usize, u32::MAX as usize),
        Err(PoolLayoutError::CapacityOverflow)
    ));
}

#[test]
fn initialized_slots_expose_spare_capacity_without_clearing_on_reuse() {
    assert_eq!(
        std::mem::size_of::<InitializedSharedPool>(),
        std::mem::size_of::<SharedPool>(),
    );
    assert_eq!(
        std::mem::size_of::<InitializedSharedLease>(),
        std::mem::size_of::<SharedLease>(),
    );

    let pool = InitializedSharedPool::new(1, 8);
    let mut lease = pool.try_acquire().expect("initialized slot");
    assert_eq!(lease.spare_mut(), &[0; 8]);
    lease.spare_mut()[..4].copy_from_slice(b"body");
    lease.try_advance(4).expect("slot capacity");
    assert_eq!(lease.as_slice(), b"body");
    drop(lease);

    let mut reused = pool.try_acquire().expect("returned initialized slot");
    assert_eq!(&reused.spare_mut()[..4], b"body");
    reused.spare_mut()[..3].copy_from_slice(b"new");
    reused.try_advance(3).expect("slot capacity");
    assert_eq!(reused.freeze().as_ref(), b"new");
}
