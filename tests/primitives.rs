use std::pin::pin;

use o3::buffer::{Lease, Pool};
use o3::cell::{BrandCell, BrandToken};
use o3::mem::ByteBudget;
use o3::mem::Scratch;

#[test]
fn pooled_buffers_enforce_capacity_and_recycle_leases() {
    assert_eq!(std::mem::size_of::<Lease<'static>>(), 32);
    let pool = pin!(Pool::new(1, 4));
    let mut buffer = pool.as_ref().try_acquire().unwrap();
    buffer.try_extend_from_slice(b"abcd").unwrap();
    let overflow = buffer.try_push(b'e').unwrap_err();
    assert_eq!(buffer.as_ref(), b"abcd");
    assert_eq!(overflow.attempted(), 5);
    assert_eq!(overflow.capacity(), 4);
    assert!(pool.as_ref().try_acquire().is_none());
    drop(buffer);
    assert_eq!(pool.available(), 1);
}

#[test]
fn byte_budget_returns_capacity() {
    let budget = pin!(ByteBudget::new(4));
    let handle = budget.as_ref().handle();
    let mut lease = handle.try_acquire(2).unwrap();
    assert!(lease.try_grow(1));
    assert!(handle.try_acquire(2).is_none());
    lease.shrink(1);
    assert_eq!(lease.amount(), 2);
    drop(lease);
    assert_eq!(handle.used(), 0);
}

#[test]
fn pooled_buffers_extend_from_slices_with_one_reservation() {
    let pool = std::pin::pin!(Pool::new(1, 8));
    let mut lease = pool.as_ref().try_acquire().unwrap();
    lease
        .try_extend_from_slices([&b"ab"[..], &b"cde"[..]])
        .unwrap();
    assert_eq!(lease.as_ref(), b"abcde");
    assert!(
        lease
            .try_extend_from_slices([&b"fg"[..], &b"hi"[..]])
            .is_err()
    );
    assert_eq!(lease.as_ref(), b"abcde");
}

#[test]
fn pooled_buffers_reuse_consumed_prefixes() {
    let pool = std::pin::pin!(Pool::new(1, 5));
    let mut lease = pool.as_ref().try_acquire().unwrap();
    lease.spare_writer().try_extend_from_slice(b"abcd").unwrap();
    lease.consume(3);
    lease.try_extend_from_slice(b"efgh").unwrap();
    assert_eq!(lease.as_ref(), b"defgh");
    assert!(lease.try_push(b'i').is_err());
    assert_eq!(lease.as_ref(), b"defgh");
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
fn scratch_reuses_only_its_vector_storage() {
    let scratch = Scratch::new();
    let mut value = Vec::with_capacity(8);
    value.push(1u8);
    scratch.put(value);
    assert!(scratch.take().capacity() >= 8);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(std::mem::size_of::<Scratch<u8>>(), 24);
}
