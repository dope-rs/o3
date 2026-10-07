use o3::mem::{self, slot::Vacant};
use std::mem::MaybeUninit;

fn main() {
    let mut backing = MaybeUninit::uninit();
    let mut escaped = None;
    Vacant::new(&mut backing).insert(0).with_pin(|value| {
        escaped = Some(value);
    });
    drop(escaped);
}

fn after_take<T: Copy>(slot: mem::raw::Occupied<'_, T>) -> T {
    let value = slot.get();
    let _ = slot.take();
    *value
}
