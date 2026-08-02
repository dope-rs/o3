use o3::permit::{ReturnPermit, ReturnTo};

struct Sink;

impl ReturnTo for Sink {
    type Item = ();

    fn return_item(&self, _: Self::Item) {}
}

fn main() {
    let permit = ReturnPermit::new(Sink, ());
    let borrow = &permit;
    let _item = permit.into_item();
    let _ = borrow;
}
