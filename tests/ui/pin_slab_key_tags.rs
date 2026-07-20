use o3::collections::{PinSlab, SlabKey};

struct Read;
struct Write;

fn remove(slab: &mut PinSlab<u8, Write>, key: SlabKey<Read>) {
    slab.remove(key);
}

fn main() {}
