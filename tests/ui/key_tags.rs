use o3::collections::{SlabKey, Slab};

struct Read;
struct Write;

fn remove(slab: &mut Slab<u8, Write>, key: SlabKey<Read>) {
    slab.remove(key);
}

fn main() {}
