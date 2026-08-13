use std::cell::Cell;

use o3::permit::{Lease, Return};

struct Sink<'a>(&'a Cell<Option<u8>>);

impl Return for Sink<'_> {
    type Item = u8;

    fn return_item(&self, item: u8) {
        assert_eq!(self.0.replace(Some(item)), None);
    }
}

#[test]
fn drop_returns_the_item() {
    let returned = Cell::new(None);
    {
        let _permit = Lease::new(Sink(&returned), 7);
    }
    assert_eq!(returned.get(), Some(7));
}

#[test]
fn accessors_borrow_the_sink_and_item() {
    let returned = Cell::new(None);
    let permit = Lease::new(Sink(&returned), 7);
    assert!(std::ptr::eq(permit.sink().0, &returned));
    assert_eq!(*permit.item(), 7);
    drop(permit);
    assert_eq!(returned.get(), Some(7));
}

#[test]
fn into_parts_disarms_the_return() {
    let returned = Cell::new(None);
    let permit = Lease::new(Sink(&returned), 7);
    assert_eq!(
        std::mem::size_of_val(&permit),
        std::mem::size_of::<(Sink<'_>, u8)>()
    );
    let (sink, item) = permit.into_parts();
    assert!(std::ptr::eq(sink.0, &returned));
    assert_eq!(item, 7);
    assert_eq!(returned.get(), None);
}
