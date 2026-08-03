use std::marker::PhantomPinned;

use o3::cell::{BrandCell, BrandToken};

struct Pinned(PhantomPinned);

fn main() {
    BrandToken::scope(|mut token| {
        let cell = BrandCell::new(Pinned(PhantomPinned));
        let _ = cell.borrow_mut(&mut token);
    });
}
