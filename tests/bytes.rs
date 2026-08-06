use std::mem::{needs_drop, size_of};

use o3::buffer::{self, Borrowed, Bytes, RetainBytes, Retained, Shared};

fn span(value: &impl RetainBytes) -> &[u8] {
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
    let bytes = Bytes::<Borrowed<'_>>::from(b"abcdef").get(1..5).unwrap();
    assert_eq!(bytes.as_slice(), b"bcde");
    assert_eq!(bytes.get(1..3).unwrap().as_slice(), b"cd");
}

#[test]
fn borrowed_retention_is_an_explicit_copy() {
    let source = [1, 2, 3, 4];
    let retained = RetainBytes::into_retained(Bytes::<Borrowed<'_>>::from(&source));
    assert_eq!(retained.as_slice(), source);
}

#[test]
fn pooled_ranges_enter_owned_storage_directly() {
    let pool = buffer::Pool::<buffer::Uninitialized>::try_new(1, 16).unwrap();
    let mut lease = pool.try_acquire().expect("pool slot");
    lease.try_extend(b"abcdef").expect("slot capacity");
    let source = lease.as_slice()[1..5].as_ptr();

    let retained = Bytes::<Retained>::from(lease.freeze()).get(1..5).unwrap();
    assert_eq!(retained.as_slice(), b"bcde");
    assert_eq!(retained.as_slice().as_ptr(), source);
    assert_eq!(pool.available(), 0);
    drop(retained);
    assert_eq!(pool.available(), 1);
}

#[test]
fn empty_pooled_slice_releases_its_slot() {
    let pool = buffer::Pool::<buffer::Uninitialized>::try_new(1, 8).unwrap();
    let mut lease = pool.try_acquire().expect("pool slot");
    lease.try_extend(b"abcdef").expect("slot capacity");

    let bytes = Bytes::<Retained>::from(lease.freeze());
    assert_eq!(pool.available(), 0);
    let empty = bytes.get(3..3).unwrap();
    assert!(empty.is_empty());
    assert_eq!(pool.available(), 1);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn retained_storage_stays_compact() {
    assert_eq!(size_of::<Bytes<Retained>>(), 32);
}

#[test]
fn retained_storage_advances_and_slices_without_copying() {
    let pool = buffer::Pool::<buffer::Uninitialized>::try_new(1, 8).unwrap();
    let mut lease = pool.try_acquire().expect("pool slot");
    lease.try_extend(b"abcdef").expect("slot capacity");
    let source = lease.as_slice().as_ptr();
    let mut retained = Bytes::<Retained>::from(lease.freeze()).get(1..5).unwrap();

    assert_eq!(retained.as_slice(), b"bcde");
    assert_eq!(retained.as_slice().as_ptr(), source.wrapping_add(1));
    assert_eq!(pool.available(), 0);
    assert!(retained.try_advance(1));
    assert_eq!(retained.as_slice(), b"cde");
    assert_eq!(retained.as_slice().as_ptr(), source.wrapping_add(2));
    drop(retained);
    assert_eq!(pool.available(), 1);
}

#[test]
fn latest_byte_policies_reject_invalid_ranges() {
    let shared = Shared::copy_from_slice(b"abcdef");
    let range = |start, end| start..end;
    let invalid = [
        Bytes::<Borrowed<'_>>::from(b"abc")
            .get(range(2, 1))
            .is_none(),
        Bytes::<Retained>::from(shared.clone()).get(1..7).is_none(),
        Bytes::<Borrowed<'_>>::from(b"abcdef")
            .get(1..4)
            .unwrap()
            .get(0..4)
            .is_none(),
        Bytes::<Retained>::from(shared).get(0..7).is_none(),
    ];
    assert!(invalid.into_iter().all(|rejected| rejected));
}
