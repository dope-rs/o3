use std::mem::MaybeUninit;

use o3::depot::array::init::Prefix;

fn main() {
    let mut first = [MaybeUninit::<u8>::uninit(); 1];
    let mut second = [MaybeUninit::<u8>::uninit(); 1];
    Prefix::<u8, 1>::with(&mut first, |first| {
        let Some(slot) = first.vacancy() else {
            return;
        };
        let key = slot.insert(1);
        Prefix::<u8, 1>::with(&mut second, |second| {
            let Some(slot) = second.vacancy() else {
                return;
            };
            slot.insert(2);
            let _ = second.get(key);
        });
    });
}
