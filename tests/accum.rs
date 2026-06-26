use o3::buffer::Accum;

#[test]
fn accumulate_consume_grow_and_cap() {
    let mut a = Accum::<{ 1 << 20 }>::new();
    assert!(a.is_empty());
    assert!(a.extend(b"hello "));
    assert!(a.extend(b"world"));
    assert_eq!(a.len(), 11);
    assert_eq!(&*a.peek().unwrap(), b"hello world");

    a.advance(6);
    assert_eq!(&*a.peek().unwrap(), b"world");

    // Grow past the 16 KiB initial capacity, then drain and compact back empty.
    let chunk = vec![7u8; 20 * 1024];
    assert!(a.extend(&chunk));
    assert_eq!(a.len(), 5 + chunk.len());
    a.advance(a.len());
    a.compact();
    assert!(a.is_empty());
    assert!(a.peek().is_none());

    // The hard cap rejects oversized input instead of growing without bound.
    let mut small = Accum::<{ 16 * 1024 }>::new();
    assert!(!small.extend(&vec![0u8; 16 * 1024 + 1]));
}
