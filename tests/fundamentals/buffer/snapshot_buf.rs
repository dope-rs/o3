use o3::buffer::{PrefixConsumer, view::Snapshot};

#[test]
fn append_consume_grow_and_enforce_capacity() {
    let mut buf = Snapshot::<{ 1 << 20 }>::with_capacity_up_to(16 * 1024);
    assert!(buf.is_empty());
    assert!(buf.try_extend(b"hello ").is_ok());
    assert!(buf.try_extend(b"world").is_ok());
    assert_eq!(buf.len(), 11);
    assert_eq!(&*buf.snapshot().unwrap(), b"hello world");

    buf.try_consume_prefix(6).unwrap().commit();
    assert_eq!(&*buf.snapshot().unwrap(), b"world");

    let chunk = vec![7u8; 20 * 1024];
    assert!(buf.try_extend(&chunk).is_ok());
    assert_eq!(buf.len(), 5 + chunk.len());
    buf.consume_prefix_up_to(buf.len());
    buf.compact();
    assert!(buf.is_empty());
    assert!(buf.snapshot().is_none());

    let mut small = Snapshot::<{ 16 * 1024 }>::with_capacity_up_to(16 * 1024);
    assert!(small.try_extend(&vec![0u8; 16 * 1024 + 1]).is_err());
}

#[test]
fn snapshots_preserve_shared_ranges_across_mutation() {
    let mut buf = Snapshot::<{ 1 << 20 }>::with_capacity_up_to(16 * 1024);
    assert!(buf.try_extend(b"before").is_ok());
    let snapshot = buf.snapshot().unwrap();
    assert!(buf.try_extend(b" after").is_ok());
    let current = buf.snapshot().unwrap();
    assert_eq!(&*snapshot, b"before");
    assert_eq!(&*current, b"before after");
    assert_eq!(snapshot.as_ptr(), current.as_ptr());

    let mut buf = Snapshot::<{ 1 << 20 }>::with_capacity_up_to(16 * 1024);
    assert!(buf.try_extend(b"before").is_ok());
    let snapshot = buf.snapshot().unwrap();
    buf.consume_prefix_up_to(buf.len());
    buf.compact();
    assert!(buf.try_extend(b"later").is_ok());
    let later = buf.snapshot().unwrap();
    assert_eq!(&*snapshot, b"before");
    assert_eq!(&*later, b"later");
    assert_eq!(
        unsafe { snapshot.as_ptr().add(snapshot.len()) },
        later.as_ptr()
    );

    let mut buf = Snapshot::<{ 1 << 20 }>::with_capacity_up_to(16 * 1024);
    assert!(buf.try_extend(b"discardlive").is_ok());
    buf.try_consume_prefix(7).unwrap().commit();
    let snapshot = buf.snapshot().unwrap();
    assert!(buf.try_extend(b"!").is_ok());
    let current = buf.snapshot().unwrap();
    assert_eq!(&*snapshot, b"live");
    assert_eq!(&*current, b"live!");
    assert_eq!(snapshot.as_ptr(), current.as_ptr());
}
