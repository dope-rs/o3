use o3::collections::{LeaseSlab, SlabLease};

fn escape() -> SlabLease<'static, u8> {
    let slab = LeaseSlab::try_with_capacity(1).unwrap();
    slab.insert(1).unwrap()
}

fn main() {
    let _lease = escape();
}
