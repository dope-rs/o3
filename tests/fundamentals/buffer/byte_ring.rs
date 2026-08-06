use std::num::NonZeroUsize;

use o3::buffer::{PrefixConsumer, queue::Ring};

#[test]
fn wraps_without_moving_live_bytes() {
    let mut ring = Ring::with_capacity(NonZeroUsize::new(8).unwrap());
    ring.try_extend(b"abcdef").unwrap();
    ring.try_consume_prefix(5).unwrap().commit();
    ring.try_extend(b"ghijkl").unwrap();

    let (first, second) = ring.as_slices();
    assert_eq!(first, b"fgh");
    assert_eq!(second, b"ijkl");
    ring.try_consume_prefix(7).unwrap().commit();
    assert_eq!(ring.as_slices(), (&[][..], &[][..]));
}

#[test]
fn capacity_is_a_hard_bound() {
    let mut ring = Ring::with_capacity(NonZeroUsize::new(4).unwrap());
    ring.try_extend(b"1234").unwrap();
    assert!(ring.try_extend(b"5").is_err());
    assert_eq!(ring.as_slices(), (&b"1234"[..], &[][..]));
}
