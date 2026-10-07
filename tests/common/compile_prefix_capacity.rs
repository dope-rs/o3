use std::mem::MaybeUninit;

use o3::depot::array::init::Prefix;

fn main() {
    let mut backing = [MaybeUninit::<u8>::uninit(); 257];
    Prefix::<u8, 257>::with(&mut backing, |_| {});
}
