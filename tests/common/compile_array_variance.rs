use std::mem::MaybeUninit;

use o3::alloc::{
    self,
    array::{Block, Slot, Written},
};

fn put<'slot, 'value>(
    slot: Slot<'slot, &'value u8>,
    value: &'value u8,
) -> Result<Written<'slot>, ()> {
    Ok(slot.insert(value))
}

fn main() -> Result<(), alloc::Error> {
    let block = Block::<MaybeUninit<&u8>, _>::try_new(1usize, ())?;
    let _ = block.try_fill(|slot| {
        let value = 7;
        put(slot, &value)
    });
    Ok(())
}
