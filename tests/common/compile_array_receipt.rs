use std::mem::MaybeUninit;

use o3::alloc::{self, array::Block};

fn main() -> Result<(), alloc::Error> {
    let outer = Block::<MaybeUninit<u8>, _>::try_new(1usize, ())?;
    let _ = outer.try_fill(|slot| {
        let mut done = Some(slot.insert(1));
        let inner = Block::<MaybeUninit<u8>, _>::try_new(1usize, ()).map_err(|_| ())?;
        let _ = inner.try_fill(|_| done.take().ok_or(()));
        Err::<_, ()>(())
    });
    Ok(())
}
