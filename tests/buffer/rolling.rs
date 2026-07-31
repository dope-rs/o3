use o3::buffer::RollingBuffer;

#[test]
fn push_consume_compact_zero_copy() {
    let mut r: RollingBuffer<8> = RollingBuffer::new();
    r.try_extend_from_slice(b"hello").unwrap();
    assert_eq!(r.as_slice(), b"hello");
    assert_eq!(r.len(), 5);
    assert_eq!(r.spare_capacity(), 3);

    r.try_consume(3).unwrap();
    assert_eq!(r.as_slice(), b"lo");
    assert_eq!(r.spare_capacity(), 6);

    r.try_extend_from_slice(b"world!").unwrap();
    assert_eq!(r.as_slice(), b"loworld!");
    assert_eq!(r.len(), 8);
    assert_eq!(r.spare_capacity(), 0);

    r.try_consume(8).unwrap();
    assert!(r.is_empty());
    assert_eq!(r.spare_capacity(), 8);

    let mut spare = r.spare_writer();
    assert_eq!(spare.capacity(), 8);
    spare.try_extend_from_slice(b"abcd").unwrap();
    spare.finish();
    assert_eq!(r.as_slice(), b"abcd");

    assert!(r.try_consume(5).is_err());
    assert_eq!(r.as_slice(), b"abcd");

    let mut boxed = RollingBuffer::<8>::new_boxed();
    boxed.try_extend_from_slice(b"boxed").unwrap();
    assert_eq!(boxed.as_slice(), b"boxed");
}
