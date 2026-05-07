use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Wake, Waker};

use o3::task::WakerSet;

struct Counter(AtomicU32);

impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn dedups_register_and_wakes_once_via_take() {
    let a = Arc::new(Counter(AtomicU32::new(0)));
    let b = Arc::new(Counter(AtomicU32::new(0)));
    let wa: Waker = a.clone().into();
    let wb: Waker = b.clone().into();

    let mut set = WakerSet::new();
    set.register(&wa);
    set.register(&wa);
    set.register(&wb);
    assert_eq!(set.len(), 2);

    let taken = set.take();
    assert!(set.is_empty());
    taken.wake_all();

    assert_eq!(a.0.load(Ordering::Relaxed), 1);
    assert_eq!(b.0.load(Ordering::Relaxed), 1);
}
