pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use common::alloc::system::Tracker;
use o3::buffer::{
    bytes::window::Snapshot,
    storage,
    write::{Sink as _, consume::Consume as _},
};

#[test]
fn lazy_unique_suffix() {
    let (mut window, cold) = Tracker::measure(Snapshot::<8>::new);
    assert_eq!(cold, [0; 4]);
    window.write_slice(b"abcdef").expect("first allocation");
    let pointer = window.as_ref()[2..].as_ptr();
    window.try_consume(2).expect("prefix");
    let (edited, hot) = Tracker::measure(|| window.try_as_mut().map(|bytes| bytes[0] = b'C'));
    edited.expect("unique suffix");
    assert_eq!(hot, [0; 4]);
    assert_eq!(window.as_ref().as_ptr(), pointer);
    assert_eq!(window.as_ref(), b"Cdef");
}

#[test]
fn mutation_isolates() {
    let mut window = Snapshot::<8>::new();
    window.write_slice(b"abcdef").expect("initial prefix");
    window.try_consume(2).expect("prefix");
    let retained = window.try_snapshot().expect("retain").expect("suffix");
    window.try_as_mut().expect("copy on write")[0] = b'C';
    assert_eq!(retained.as_ref(), b"cdef");
    assert_eq!(window.as_ref(), b"Cdef");
    let retained = window.try_snapshot().expect("retain").expect("suffix");
    window.clear();
    window.write_slice(b"new").expect("append after clear");
    assert_eq!(retained.as_ref(), b"Cdef");
    assert_eq!(window.as_ref(), b"new");
}

#[test]
fn mutation_failure_inert() {
    let mut window = Snapshot::<8>::new();
    window.write_slice(b"abcdef").expect("initial prefix");
    window.try_consume(2).expect("prefix");
    let retained = window.try_snapshot().expect("retain").expect("suffix");
    Tracker::fail_after(0, || {
        assert!(matches!(
            window.try_as_mut(),
            Err(storage::AllocError::Exhausted(o3::alloc::Exhausted))
        ));
    });
    assert_eq!(window.as_ref().as_ptr(), retained.as_ptr());
    assert_eq!(window.as_ref(), b"cdef");
}
