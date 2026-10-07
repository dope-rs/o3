pub mod common;

use std::mem;

use common::depot::Index;
use o3::depot::{array::bits, raw::BitIndex as _};

#[test]
fn retains_native_domain() {
    assert_eq!(mem::size_of::<bits::Set<Index, 4>>(), 2);
    assert_eq!(mem::align_of::<bits::Set<Index, 4>>(), 2);
    for mask in 0_u16..16 {
        let mut set = Index::from_mask(mask);
        assert_eq!(set.len().get(), mask.count_ones() as usize);
        assert_eq!(set.is_empty(), mask == 0);
        assert_eq!(set.complement().len().get(), 4 - set.len().get());
        let mut left = mask;
        while let Some(index) = set.first() {
            assert_eq!(index.index(), left.trailing_zeros() as usize);
            assert_eq!(
                o3::depot::array::Index::<4>::from_bit(index).index(),
                index.index()
            );
            assert!(set.contains(index));
            set.remove(index);
            assert!(!set.contains(index));
            left &= left - 1;
            assert_eq!(set.len().get(), left.count_ones() as usize);
        }
        assert_eq!(left, 0);
    }
}

#[test]
fn preserves_set_operations() {
    for left in 0_u16..16 {
        for right in 0_u16..16 {
            let a = Index::from_mask(left);
            let b = Index::from_mask(right);
            for (mut set, expected) in [
                (a.union(b), left | right),
                (a.intersect(b), left & right),
                (a.without(b), left & !right),
            ] {
                let mut actual = 0_u16;
                for _ in 0..4 {
                    if let Some(index) = set.first() {
                        actual |= 1 << index.index();
                        set.remove(index);
                    }
                }
                assert_eq!((actual, set.is_empty()), (expected, true));
            }
        }
    }
}
