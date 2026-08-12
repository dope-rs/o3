use std::{cell::Cell, mem};

use o3::collections::fixed::array::{CopyInline, Inline};

#[test]
fn array_vec_keeps_values_inline_and_in_order() {
    let values: Inline<_, 4> = Inline::from_fn(3, |index| index + 1);
    assert_eq!(
        mem::size_of::<Inline<u64, 4>>(),
        5 * mem::size_of::<usize>()
    );
    assert_eq!(values.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn copy_array_clone_preserves_the_initialized_prefix() {
    #[repr(C)]
    struct CompactBytes {
        entries: [u8; 65],
        len: u32,
    }

    let mut values = CopyInline::<_, 4>::new();
    values.push(3).unwrap();
    values.push(5).unwrap();

    assert_eq!(values.clone().as_slice(), [3, 5]);
    assert!(!mem::needs_drop::<CopyInline<u64, 4>>());
    assert_eq!(
        mem::size_of::<CopyInline<u64, 4>>(),
        mem::size_of::<[u64; 4]>() + mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<CopyInline<u8, 65>>(),
        mem::size_of::<CompactBytes>()
    );
}

#[test]
fn copy_array_mutation_is_bounded_and_atomic() {
    let mut values = CopyInline::<_, 4>::new();
    values.push(3).unwrap();
    values.push(7).unwrap();
    values.insert(1, 5).unwrap();
    assert_eq!(values.as_slice(), [3, 5, 7]);

    assert_eq!(values.try_extend_from_slice(&[9, 11]), Err(&[9, 11][..]));
    assert_eq!(values.as_slice(), [3, 5, 7]);

    assert_eq!(values.insert(4, 13), Err(13));
    assert_eq!(values.as_slice(), [3, 5, 7]);

    values.as_mut_slice()[1] = 6;
    assert_eq!(values.pop(), Some(7));
    assert_eq!(values.as_slice(), [3, 6]);
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
        drop(Inline::<_, 2>::from_fn(2, |_| PanicDrop {
            drops: &drops,
            panic_once: &panic_once,
        }));
    }));
    assert!(result.is_err());
    assert_eq!(drops.get(), 2);
}

#[test]
fn owned_array_truncation_releases_the_exact_suffix_after_a_drop_panic() {
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
    let mut values = Inline::<_, 3>::from_fn(3, |_| PanicDrop {
        drops: &drops,
        panic_once: &panic_once,
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        values.truncate(0);
    }));
    assert!(result.is_err());
    assert_eq!(drops.get(), 3);
    assert!(values.is_empty());
}
