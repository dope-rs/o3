use std::mem::MaybeUninit;

use o3::depot::array::init::Prefix;

#[repr(align(2))]
struct Aligned([u8; 2]);

fn main() {
    let mut backing = [MaybeUninit::<Aligned>::uninit()];
    Prefix::<[u8; 2], 1>::with(&mut backing, |_| {});
}
