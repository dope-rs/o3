use o3::depot::{
    arena::slab::{self, storage},
    capacity::Count,
};

fn main() {
    slab::Pinned::<u8, (), 0, false, 1, storage::Heap, storage::Lazy>::try_new(Count::EMPTY)
        .expect("slab");
}
