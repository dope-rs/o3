use o3::buffer::{
    segments::{self, queue},
    storage,
};

const _: fn(&mut queue::Fifo<storage::Shared>, segments::Item<storage::Shared>) = |queue, item| {
    use o3::depot::raw::RingEdit as _;

    queue.slices_mut().0[0] = item;
};

fn main() {}
