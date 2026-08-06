use o3::collections::{LeaseSlab, SlabCapacity, SlabLease};

fn escape() -> SlabLease<'static, u8> {
    let slab = LeaseSlab::with_capacity(SlabCapacity::new(1));
    slab.vacant_entry().unwrap().insert(1)
}

fn main() {
    let _lease = escape();
}
