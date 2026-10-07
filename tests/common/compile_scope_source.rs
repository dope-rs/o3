use std::cell::Cell;

use o3::buffer::bytes::scope;

struct Changing(Cell<bool>);

impl AsRef<[u8]> for Changing {
    fn as_ref(&self) -> &[u8] {
        if self.0.replace(!self.0.get()) {
            b"x"
        } else {
            b"xy"
        }
    }
}

fn main() {
    scope::Owner::with(Changing(Cell::new(false)), |owner| {
        let _ = owner.view();
    });
    let mut source = Changing(Cell::new(false));
    scope::Owner::with(&source, |owner| {
        let _ = owner.borrow();
    });
    scope::Owner::with(&mut source, |owner| {
        let _ = owner.borrow();
    });
}
