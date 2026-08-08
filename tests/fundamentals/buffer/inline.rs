use std::mem::size_of;

use o3::buffer::storage::inline::{Bytes, CAPACITY};

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
