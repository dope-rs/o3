use o3::buffer::SnapshotBuf;

#[test]
fn append_consume_grow_and_enforce_capacity() {
    let mut buf = SnapshotBuf::<{ 1 << 20 }>::with_capacity(16 * 1024);
    assert!(buf.is_empty());
    assert!(buf.try_extend_from_slice(b"hello ").is_ok());
    assert!(buf.try_extend_from_slice(b"world").is_ok());
    assert_eq!(buf.len(), 11);
    assert_eq!(&*buf.snapshot().unwrap(), b"hello world");

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        buf.advance(usize::MAX);
    }));
    assert!(caught.is_err());
    assert_eq!(buf.len(), 11);

    buf.advance(6);
    assert_eq!(&*buf.snapshot().unwrap(), b"world");

    let chunk = vec![7u8; 20 * 1024];
    assert!(buf.try_extend_from_slice(&chunk).is_ok());
    assert_eq!(buf.len(), 5 + chunk.len());
    buf.advance(buf.len());
    buf.compact();
    assert!(buf.is_empty());
    assert!(buf.snapshot().is_none());

    let mut small = SnapshotBuf::<{ 16 * 1024 }>::with_capacity(16 * 1024);
    assert!(
        small
            .try_extend_from_slice(&vec![0u8; 16 * 1024 + 1])
            .is_err()
    );
}

#[test]
fn snapshots_preserve_shared_ranges_across_mutation() {
    let mut buf = SnapshotBuf::<{ 1 << 20 }>::with_capacity(16 * 1024);
    assert!(buf.try_extend_from_slice(b"before").is_ok());
    let snapshot = buf.snapshot().unwrap();
    assert!(buf.try_extend_from_slice(b" after").is_ok());
    let current = buf.snapshot().unwrap();
    assert_eq!(&*snapshot, b"before");
    assert_eq!(&*current, b"before after");
    assert_eq!(snapshot.as_ptr(), current.as_ptr());

    let mut buf = SnapshotBuf::<{ 1 << 20 }>::with_capacity(16 * 1024);
    assert!(buf.try_extend_from_slice(b"before").is_ok());
    let snapshot = buf.snapshot().unwrap();
    buf.advance(buf.len());
    buf.compact();
    assert!(buf.try_extend_from_slice(b"later").is_ok());
    let later = buf.snapshot().unwrap();
    assert_eq!(&*snapshot, b"before");
    assert_eq!(&*later, b"later");
    assert_eq!(
        unsafe { snapshot.as_ptr().add(snapshot.len()) },
        later.as_ptr()
    );

    let mut buf = SnapshotBuf::<{ 1 << 20 }>::with_capacity(16 * 1024);
    assert!(buf.try_extend_from_slice(b"discardlive").is_ok());
    buf.advance(7);
    let snapshot = buf.snapshot().unwrap();
    assert!(buf.try_extend_from_slice(b"!").is_ok());
    let current = buf.snapshot().unwrap();
    assert_eq!(&*snapshot, b"live");
    assert_eq!(&*current, b"live!");
    assert_eq!(snapshot.as_ptr(), current.as_ptr());
}
