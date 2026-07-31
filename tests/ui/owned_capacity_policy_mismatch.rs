use o3::buffer::{BLOCK_CAPACITY, Owned};

fn main() {
    let owned = Owned::try_with_capacity(16).unwrap();
    let _: Owned<BLOCK_CAPACITY> = owned;
}
