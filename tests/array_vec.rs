use std::cell::Cell;
use std::mem::size_of;

use o3::collections::ArrayVec;

#[test]
fn array_vec_keeps_values_inline_and_in_order() {
    let values: ArrayVec<_, 4> = ArrayVec::from_fn(3, |index| index + 1);
    assert_eq!(values.len(), 3);
    assert_eq!(values.capacity(), 4);
    assert_eq!(size_of::<ArrayVec<u64, 4>>(), 5 * size_of::<usize>());
    assert_eq!(values.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn array_vec_releases_every_value_when_drop_panics() {
    struct PanicDrop<'a> {
        drops: &'a Cell<usize>,
        panic_once: &'a Cell<bool>,
    }

    impl Drop for PanicDrop<'_> {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            if self.panic_once.replace(false) {
                panic!("drop panic");
            }
        }
    }

    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(ArrayVec::<_, 2>::from_fn(2, |_| PanicDrop {
            drops: &drops,
            panic_once: &panic_once,
        }));
    }));
    assert!(result.is_err());
    assert_eq!(drops.get(), 2);
}
