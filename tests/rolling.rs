use o3::buffer::RollingBuffer;

#[test]
fn push_consume_compact_zero_copy() {
    let mut r: RollingBuffer<8> = RollingBuffer::new();
    r.extend_from_slice(b"hello");
    assert_eq!(r.as_slice(), b"hello");
    assert_eq!(r.len(), 5);
    assert_eq!(r.spare_capacity(), 3);

    r.consume(3);
    assert_eq!(r.as_slice(), b"lo");
    assert_eq!(r.spare_capacity(), 6);

    r.extend_from_slice(b"world!");
    assert_eq!(r.as_slice(), b"loworld!");
    assert_eq!(r.len(), 8);
    assert_eq!(r.spare_capacity(), 0);

    r.consume(8);
    assert!(r.is_empty());
    assert_eq!(r.spare_capacity(), 8);

    let mut spare = r.spare_writer();
    assert_eq!(spare.capacity(), 8);
    spare.try_extend_from_slice(b"abcd").unwrap();
    spare.finish();
    assert_eq!(r.as_slice(), b"abcd");

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.consume(5)));
    assert!(caught.is_err());
    assert_eq!(r.as_slice(), b"abcd");

    let mut boxed = RollingBuffer::<8>::new_boxed();
    boxed.extend_from_slice(b"boxed");
    assert_eq!(boxed.as_slice(), b"boxed");
}
