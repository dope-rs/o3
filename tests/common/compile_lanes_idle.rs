use o3::depot::{self, array::Index, queue::indexed::lanes::typed};

fn cannot_link_idle<K: depot::raw::Key>(slot: typed::Slot<'_, K, u8, u16, u32, typed::Idle, 2>) {
    slot.move_to_back(Index::at::<0>());
}

fn main() {}
