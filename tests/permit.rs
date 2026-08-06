use std::cell::Cell;

use o3::{ReturnPermit, ReturnTo};

struct Sink<'a>(&'a Cell<Option<u8>>);

impl ReturnTo for Sink<'_> {
    type Item = u8;

    fn return_item(&self, item: u8) {
        assert_eq!(self.0.replace(Some(item)), None);
    }
}

#[test]
fn drop_returns_the_item() {
    let returned = Cell::new(None);
    {
        let _permit = ReturnPermit::new(Sink(&returned), 7);
    }
    assert_eq!(returned.get(), Some(7));
}
