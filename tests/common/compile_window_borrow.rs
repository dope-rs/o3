use std::num::NonZero;

use o3::{base::num::bounded, buffer::bytes::scope};

fn keeps_parts(bytes: &mut [u8], at: NonZero<usize>) {
    let Some(mut window) = scope::raw::Window::<2, 3>::try_at(bytes, at) else {
        return;
    };
    let (_, number, _) = window.parts();
    let _ = window.split(bounded::U64::<0, 1>::mask(0));
    number[0] = 1;
}

fn main() {}
