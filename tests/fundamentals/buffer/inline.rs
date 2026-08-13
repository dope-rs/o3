use std::mem::size_of;

use o3::buffer::storage::inline::{Bytes, CAPACITY, Str, WideBytes};

#[test]
fn inline_bytes_fill_without_growing_the_owner() {
    let bytes = [b'x'; CAPACITY];
    let inline = Bytes::<CAPACITY>::from_slice(&bytes).unwrap();

    assert_eq!(inline.as_slice(), bytes);
    assert_eq!(inline.as_slice().len(), CAPACITY);
    assert_eq!(size_of::<Bytes>(), CAPACITY + 1);
    assert!(Bytes::<CAPACITY>::from_slice(&[0; CAPACITY + 1]).is_err());
}

#[test]
fn inline_bytes_report_capacity_before_writing_past_the_array() {
    let mut inline = Bytes::<CAPACITY>::new();
    for byte in 0..CAPACITY as u8 {
        inline.try_push(byte).unwrap();
    }

    let error = inline.try_push(0).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "capacity exceeded: attempted {}, capacity {}",
            CAPACITY + 1,
            CAPACITY
        )
    );
}

#[test]
fn inline_bytes_capacity_is_selected_by_the_type() {
    let mut inline = Bytes::<4>::from_slice(b"ab").unwrap();
    inline.try_extend(b"cd").unwrap();

    assert_eq!(inline.as_slice(), b"abcd");
    assert_eq!(size_of::<Bytes<4>>(), 5);

    let error = inline.try_extend(b"e").unwrap_err();
    assert_eq!(
        error.to_string(),
        "capacity exceeded: attempted 5, capacity 4"
    );
}

#[test]
fn wide_inline_bytes_preserve_u16_layout_and_bounds() {
    let mut inline = WideBytes::<514>::new();
    inline.try_extend(&[b'x'; 513]).unwrap();
    inline.try_push(b'y').unwrap();

    assert_eq!(inline.len(), 514);
    assert!(!inline.is_empty());
    assert_eq!(inline.as_slice()[513], b'y');
    assert_eq!(size_of::<WideBytes<514>>(), 516);
    assert!(inline.try_push(0).is_err());
}

#[test]
fn inline_str_truncates_at_a_valid_utf8_boundary_without_layout_overhead() {
    let text = Str::<5>::from_str_truncated("ééé");

    assert_eq!(text.as_str(), "éé");
    assert_eq!(size_of::<Str<256>>(), size_of::<([u8; 256], usize)>());
}
