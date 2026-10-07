use std::mem::MaybeUninit;

use o3::depot::array::init::Prefix;

fn main() {
    let mut backing = [MaybeUninit::<u8>::uninit(); 1];
    let escaped = Prefix::<u8, 1>::with(&mut backing, |prefix| {
        let Some(slot) = prefix.vacancy() else {
            return Ok(());
        };
        Err::<(), _>(slot.insert(7))
    });
    drop(escaped);
}
