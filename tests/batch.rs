use std::cell::Cell;
use std::future::{Future, ready};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use o3::task::{Batch, Lazy};

#[test]
fn batch_drives_all_to_completion_in_order() {
    let mut cx = Context::from_waker(Waker::noop());
    let futs = (0..100u32).map(ready).collect();
    let mut batch = pin!(Batch::with_window(futs, 8));
    match batch.as_mut().poll(&mut cx) {
        Poll::Ready(out) => assert_eq!(out, (0..100).collect::<Vec<_>>()),
        Poll::Pending => panic!("ready futures must complete in one poll"),
    }
}

#[test]
fn lazy_defers_construction_until_poll() {
    let built = Cell::new(false);
    let lazy = Lazy::new(|| {
        built.set(true);
        ready(42)
    });
    assert!(!built.get());

    let mut cx = Context::from_waker(Waker::noop());
    let mut lazy = pin!(lazy);
    assert_eq!(lazy.as_mut().poll(&mut cx), Poll::Ready(42));
    assert!(built.get());
}
