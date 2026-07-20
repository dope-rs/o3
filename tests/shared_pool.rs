use crate::confined::assert_confined;
use o3::buffer::{Pooled, SharedLease, SharedPool};

assert_confined!(SharedPool);
assert_confined!(SharedLease);
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
