use std::mem::MaybeUninit;

use o3::alloc::{self, array::Block};

fn main() -> Result<(), alloc::Error> {
    let block = Block::<MaybeUninit<u8>, _>::try_new(1usize, ())?;
    let _ = block.try_fill(|_| Ok::<(), ()>(()));
    Ok(())
}
