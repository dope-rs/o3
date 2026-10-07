use o3::depot::queue::indexed::slab;

fn escape(vacant: slab::Vacant<'_, u8>) -> slab::Handle {
    let key = vacant.key();
    drop(vacant);
    key
}

fn main() {}
