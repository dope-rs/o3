use std::mem::size_of;

use o3::buffer::{INLINE_BYTES_CAPACITY, InlineBytes};

#[test]
fn inline_bytes_fill_without_growing_the_owner() {
    let bytes = [b'x'; INLINE_BYTES_CAPACITY];
    let inline = InlineBytes::<INLINE_BYTES_CAPACITY>::from_slice(&bytes).unwrap();

    assert_eq!(inline.as_slice(), bytes);
    assert_eq!(inline.len(), INLINE_BYTES_CAPACITY);
    assert_eq!(size_of::<InlineBytes>(), INLINE_BYTES_CAPACITY + 1);
    assert!(
        InlineBytes::<INLINE_BYTES_CAPACITY>::from_slice(&[0; INLINE_BYTES_CAPACITY + 1]).is_err()
    );
}

#[test]
fn inline_bytes_report_capacity_before_writing_past_the_array() {
    let mut inline = InlineBytes::<INLINE_BYTES_CAPACITY>::new();
    for byte in 0..INLINE_BYTES_CAPACITY as u8 {
        inline.try_push(byte).unwrap();
    }

    let error = inline.try_push(0).unwrap_err();
    assert_eq!(error.attempted(), INLINE_BYTES_CAPACITY + 1);
    assert_eq!(error.capacity(), INLINE_BYTES_CAPACITY);
}

#[test]
fn inline_bytes_capacity_is_selected_by_the_type() {
    let mut inline = InlineBytes::<4>::from_slice(b"ab").unwrap();
    inline.try_extend_from_slice(b"cd").unwrap();

    assert_eq!(inline.as_slice(), b"abcd");
    assert_eq!(InlineBytes::<4>::CAPACITY, 4);
    assert_eq!(size_of::<InlineBytes<4>>(), 5);

    let error = inline.try_extend_from_slice(b"e").unwrap_err();
    assert_eq!(error.attempted(), 5);
    assert_eq!(error.capacity(), 4);
}
