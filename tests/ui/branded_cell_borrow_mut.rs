use std::marker::PhantomPinned;

use o3::cell::{Brand, BrandToken};

struct Pinned(PhantomPinned);

fn main() {
    BrandToken::scope(|mut token| {
        let cell = Brand::new(Pinned(PhantomPinned));
        let _ = cell.borrow_mut(&mut token);
    });
}
