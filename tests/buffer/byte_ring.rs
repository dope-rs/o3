use o3::buffer::ByteRing;

#[test]
fn wraps_without_moving_live_bytes() {
    let mut ring = ByteRing::with_capacity(8);
    ring.try_extend_from_slice(b"abcdef").unwrap();
    ring.consume(5);
    ring.try_extend_from_slice(b"ghijkl").unwrap();

    let (first, second) = ring.as_slices();
    assert_eq!(first, b"fgh");
    assert_eq!(second, b"ijkl");
    assert_eq!(ring.range_slices(1, 4), Some((&b"gh"[..], &b"ij"[..])));

    ring.consume(7);
    assert!(ring.is_empty());
    assert_eq!(ring.as_slices(), (&[][..], &[][..]));
}

#[test]
fn capacity_is_a_hard_bound() {
    let mut ring = ByteRing::with_capacity(4);
    ring.try_extend_from_slice(b"1234").unwrap();
    assert!(ring.try_push(b'5').is_err());
    assert_eq!(ring.as_slices(), (&b"1234"[..], &[][..]));
}
