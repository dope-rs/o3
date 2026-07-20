use std::marker::PhantomPinned;

use o3::collections::PinSlab;

fn main() {
    let mut slab: PinSlab<PhantomPinned> = PinSlab::with_capacity(1);
    let key = slab.insert(PhantomPinned).unwrap();
    let _ = slab.take(key);
}
