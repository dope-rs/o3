use std::mem::MaybeUninit;

use o3::depot::array::init::Prefix;

fn main() {
    let mut backing = [MaybeUninit::<[u8; 2]>::uninit(); 1];
    Prefix::<u8, 1>::with(&mut backing, |_| {});
}
