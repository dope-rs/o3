use std::panic::{AssertUnwindSafe, catch_unwind};

use o3::buffer::{RawMut, Read, Shared};
use o3::mem::Mmap;

#[test]
fn get_u32_on_short_shared_panics_not_hangs() {
    let mut s = Shared::copy_from_slice(b"ab");
    let r = catch_unwind(AssertUnwindSafe(|| s.get_u32()));
    assert!(r.is_err());
}

#[test]
fn get_u64_on_short_cursor_panics_not_hangs() {
    let mut c = std::io::Cursor::new(vec![0u8, 1, 2]);
    let r = catch_unwind(AssertUnwindSafe(|| c.get_u64()));
    assert!(r.is_err());
}

#[test]
fn get_uint_on_short_shared_panics_not_hangs() {
    let mut s = Shared::copy_from_slice(b"x");
    let r = catch_unwind(AssertUnwindSafe(|| s.get_uint(4)));
    assert!(r.is_err());
}

#[test]
fn get_u32_on_exact_shared_succeeds() {
    let mut s = Shared::copy_from_slice(&[0, 0, 1, 0]);
    assert_eq!(s.get_u32(), 256);
    assert_eq!(s.remaining(), 0);
}

#[test]
fn from_raw_range_out_of_bounds_panics() {
    let raw = RawMut::with_capacity(4).freeze();
    let r = catch_unwind(AssertUnwindSafe(move || {
        Shared::from_raw_range(raw, 0, 100)
    }));
    assert!(r.is_err());
}

#[test]
fn from_raw_range_in_bounds_ok() {
    let raw = RawMut::with_capacity(8).freeze();
    let s = Shared::from_raw_range(raw, 0, 0);
    assert!(s.is_empty());
}

#[test]
fn mmap_zeroed_integer_is_zeroed() {
    let m = Mmap::<u32>::new_zeroed(16).expect("mmap");
    assert_eq!(m.len(), 16);
    assert!(m.iter().all(|&x| x == 0));

    let b = Mmap::<u8>::new_zeroed(64).expect("mmap");
    assert!(b.iter().all(|&x| x == 0));
}
