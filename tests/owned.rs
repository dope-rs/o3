use o3::buffer::{Owned, Shared};

#[test]
fn build_and_freeze() {
    let mut o = Owned::new();
    assert!(o.is_empty());
    assert_eq!(o.capacity(), 0);

    o.extend_from_slice(b"hello");
    assert_eq!(o.len(), 5);
    assert_eq!(o.as_slice(), b"hello");
    assert!(o.capacity() >= 5);

    o.extend_from_slice(b" world");
    assert_eq!(o.as_slice(), b"hello world");

    let s: Shared = o.freeze();
    assert_eq!(s.as_slice(), b"hello world");
    assert_eq!(s.clone().as_slice(), b"hello world");
}

#[test]
fn split_to_and_off() {
    let mut o = Owned::from(&b"abcdefgh"[..]);

    let head = o.split_to(3);
    assert_eq!(head.as_slice(), b"abc");
    assert_eq!(o.as_slice(), b"defgh");

    let tail = o.split_off(2);
    assert_eq!(o.as_slice(), b"de");
    assert_eq!(tail.as_slice(), b"fgh");
}

#[test]
fn split_leaves_owned_empty_and_reusable() {
    let mut o = Owned::with_capacity(16);
    o.extend_from_slice(b"first");
    let first: Shared = o.split();
    assert_eq!(first.as_slice(), b"first");
    assert!(o.is_empty());

    o.extend_from_slice(b"second");
    let second: Shared = o.freeze();
    assert_eq!(second.as_slice(), b"second");
}

#[test]
fn spare_capacity_write_then_set_len() {
    let mut o = Owned::with_capacity(8);
    let spare = o.spare_capacity_mut();
    assert_eq!(spare.len(), 8);
    spare[0].write(b'x');
    spare[1].write(b'y');
    spare[2].write(b'z');
    unsafe {
        o.set_len(3);
    }
    assert_eq!(o.as_slice(), b"xyz");
}
