use o3::buffer::{PrefixConsumer, view::window::Inline};

#[test]
fn push_consume_compact_zero_copy() {
    let mut r = Inline::<8>::default();
    r.try_extend(b"hello").unwrap();
    assert_eq!(r.as_slice(), b"hello");
    assert_eq!(r.len(), 5);
    assert_eq!(r.spare_capacity(), 3);

    r.try_consume_prefix(3).unwrap().commit();
    assert_eq!(r.as_slice(), b"lo");
    assert_eq!(r.spare_capacity(), 6);

    r.try_extend(b"world!").unwrap();
    assert_eq!(r.as_slice(), b"loworld!");
    assert_eq!(r.len(), 8);
    assert_eq!(r.spare_capacity(), 0);

    r.try_consume_prefix(8).unwrap().commit();
    assert!(r.is_empty());
    assert_eq!(r.spare_capacity(), 8);

    let mut boxed = Inline::<8>::new_boxed();
    boxed.try_extend(b"boxed").unwrap();
    assert_eq!(boxed.as_slice(), b"boxed");
}
