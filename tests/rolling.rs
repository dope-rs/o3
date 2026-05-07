use o3::buffer::Rolling;

#[test]
fn push_consume_compact_zero_copy() {
    let mut r: Rolling<8> = Rolling::new();
    r.push(b"hello");
    assert_eq!(r.as_slice(), b"hello");
    assert_eq!(r.len(), 5);
    assert_eq!(r.spare_capacity(), 3);

    r.consume(3);
    assert_eq!(r.as_slice(), b"lo");
    assert_eq!(r.spare_capacity(), 6);

    r.push(b"world!");
    assert_eq!(r.as_slice(), b"loworld!");
    assert_eq!(r.len(), 8);
    assert_eq!(r.spare_capacity(), 0);

    r.consume(8);
    assert!(r.is_empty());
    assert_eq!(r.spare_capacity(), 8);

    let spare = r.spare_capacity_mut();
    assert_eq!(spare.len(), 8);
    spare[..4].copy_from_slice(b"abcd");
    unsafe { r.advance(4); }
    assert_eq!(r.as_slice(), b"abcd");
}
