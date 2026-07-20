use o3::collections::{SlabKey, SlabKeyParts};

fn main() {
    let parts = SlabKeyParts::new(0, 1).unwrap();
    let _ = SlabKey::<()>::from_parts(parts);
}
