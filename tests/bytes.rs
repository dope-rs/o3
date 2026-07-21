use std::mem::{needs_drop, size_of};

use o3::buffer::{
    Borrowed, ByteSpan, Bytes, Leased, Pooled, RetainBytes, Retained, Shared, SharedPool,
};

fn span(value: &impl ByteSpan) -> &[u8] {
    value.as_slice()
}

#[test]
fn borrowed_bytes_are_a_transparent_slice() {
    assert_eq!(size_of::<Bytes<Borrowed<'_>>>(), size_of::<&[u8]>());
    assert!(!needs_drop::<Bytes<Borrowed<'_>>>());

    let bytes = Bytes::<Borrowed<'_>>::from(b"abcdef");
    assert_eq!(span(&bytes), b"abcdef");
    assert_eq!(bytes.len(), 6);
    assert!(!bytes.is_empty());
}

#[test]
fn borrowed_slice_stays_borrowed() {
    let bytes = Bytes::<Borrowed<'_>>::from(b"abcdef").slice(1..5);
    assert_eq!(bytes.as_slice(), b"bcde");
    assert_eq!(bytes.slice(1..3).as_slice(), b"cd");
}

#[test]
fn borrowed_retention_is_an_explicit_copy() {
    let source = [1, 2, 3, 4];
    let retained = RetainBytes::into_retained(Bytes::<Borrowed<'_>>::from(&source));
    assert_eq!(retained.as_slice(), source);
}

#[test]
fn leased_retention_transfers_the_pool_slot_without_copying() {
    assert_eq!(size_of::<Bytes<Leased>>(), size_of::<Pooled>());

    let pool = SharedPool::new(1, 16);
    let mut lease = pool.try_acquire().expect("pool slot");
    lease
        .spare_writer()
        .try_extend_from_slice(b"leased")
        .expect("slot capacity");
    let source = lease.as_slice().as_ptr();
    let bytes = Bytes::<Leased>::from(lease.freeze());

    assert_eq!(bytes.as_slice(), b"leased");
    assert_eq!(bytes.as_slice().as_ptr(), source);
    assert_eq!(pool.available(), 0);

    let retained = bytes.into_retained();
    assert_eq!(retained.as_slice().as_ptr(), source);
    assert_eq!(pool.available(), 0);
    drop(retained);
    assert_eq!(pool.available(), 1);
}

#[test]
fn pooled_ranges_enter_owned_storage_directly() {
    let pool = SharedPool::new(1, 16);
    let mut lease = pool.try_acquire().expect("pool slot");
    lease
        .spare_writer()
        .try_extend_from_slice(b"abcdef")
        .expect("slot capacity");
    let source = lease.as_slice()[1..5].as_ptr();

    let retained = Bytes::<Retained>::from(lease.freeze()).slice(1..5);
    assert_eq!(retained.as_slice(), b"bcde");
    assert_eq!(retained.as_slice().as_ptr(), source);
    assert_eq!(pool.available(), 0);
    drop(retained);
    assert_eq!(pool.available(), 1);
}

#[test]
fn empty_pooled_slice_releases_its_slot() {
    let pool = SharedPool::new(1, 8);
    let mut lease = pool.try_acquire().expect("pool slot");
    lease
        .spare_writer()
        .try_extend_from_slice(b"abcdef")
        .expect("slot capacity");

    let bytes = Bytes::<Retained>::from(lease.freeze());
    assert_eq!(pool.available(), 0);
    let empty = bytes.slice(3..3);
    assert!(empty.is_empty());
    assert_eq!(pool.available(), 1);
}

#[test]
fn shared_bytes_slice_and_retain_without_changing_storage_class() {
    assert_eq!(size_of::<Bytes<Shared>>(), size_of::<Shared>());

    let shared = Shared::copy_from_slice(b"abcdef");
    let source = shared.as_slice()[1..5].as_ptr();
    let bytes = Bytes::<Shared>::from(shared).slice(1..5);
    assert_eq!(bytes.as_slice(), b"bcde");
    assert_eq!(bytes.as_slice().as_ptr(), source);

    let retained = bytes.into_retained();
    assert_eq!(retained.as_slice(), b"bcde");
    assert_eq!(retained.as_slice().as_ptr(), source);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn retained_storage_stays_compact() {
    assert_eq!(size_of::<Bytes<Retained>>(), 32);
}

#[test]
fn retained_storage_advances_and_slices_without_copying() {
    let pool = SharedPool::new(1, 8);
    let mut lease = pool.try_acquire().expect("pool slot");
    lease
        .spare_writer()
        .try_extend_from_slice(b"abcdef")
        .expect("slot capacity");
    let source = lease.as_slice().as_ptr();
    let mut retained = Bytes::<Retained>::from(lease.freeze()).slice(1..5);

    assert_eq!(retained.as_slice(), b"bcde");
    assert_eq!(retained.as_slice().as_ptr(), source.wrapping_add(1));
    assert_eq!(pool.available(), 0);
    retained.advance(1);
    assert_eq!(retained.as_slice(), b"cde");
    assert_eq!(retained.as_slice().as_ptr(), source.wrapping_add(2));
    drop(retained);
    assert_eq!(pool.available(), 1);
}

#[test]
fn latest_byte_policies_reject_invalid_ranges() {
    let shared = Shared::copy_from_slice(b"abcdef");
    let reversed = [2, 1];
    let invalid = [
        std::panic::catch_unwind(|| {
            Bytes::<Borrowed<'_>>::from(b"abc").slice(reversed[0]..reversed[1])
        })
        .is_err(),
        std::panic::catch_unwind(|| Bytes::<Retained>::from(shared.clone()).slice(1..7)).is_err(),
        std::panic::catch_unwind(|| {
            Bytes::<Borrowed<'_>>::from(b"abcdef")
                .slice(1..4)
                .slice(0..4)
        })
        .is_err(),
        std::panic::catch_unwind(|| Bytes::<Retained>::from(shared.clone()).slice(0..7)).is_err(),
    ];
    assert!(invalid.into_iter().all(|rejected| rejected));
}
