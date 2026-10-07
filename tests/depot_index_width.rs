pub mod common;

use std::{collections::BTreeSet, mem};

use common::depot::index::{Bitmap, width::Inline};
use o3::depot::index::bitmap::{self, Access as _, entry::Row};

#[test]
fn retains_membership() {
    let mut compact = Inline::<255>::default();
    let mut flat =
        Bitmap::try_new(bitmap::Shape::<u32>::new(255), bitmap::Fill::Empty).expect("flat fixture");
    let mut model = BTreeSet::new();
    for step in 0usize..4096 {
        let index = step.wrapping_mul(73).wrapping_add(step / 7) % 255;
        if step % 3 != 0 {
            Row::try_new(&compact, index)
                .expect("compact row")
                .update::<true>(());
            Row::try_new(&flat, index)
                .expect("flat row")
                .update::<true>(());
            model.insert(index);
        } else {
            let start = usize::from(compact.cursor);
            let expected = model
                .range(start % 255..)
                .next()
                .or_else(|| model.first())
                .copied();
            let actual = (&mut compact).pop_from((), start).map(|row| row.index());
            assert_eq!(actual, expected);
            assert_eq!(
                (&mut flat).pop_from((), start).map(|row| row.index()),
                expected
            );
            if let Some(index) = expected {
                model.remove(&index);
                compact.cursor = (index as u8).wrapping_add(1);
            }
        }
        assert_eq!(compact.is_empty(()), model.is_empty());
        assert_eq!(
            (&compact).find((), index).map(|row| row.index()),
            model.range(index..).next().copied()
        );
        assert_eq!(
            Row::try_new(&compact, index).expect("row").contains(()),
            model.contains(&index)
        );
    }
    for index in model {
        let occupied = compact.first(()).expect("remaining compact member");
        assert_eq!(occupied.remove().index(), index);
        assert_eq!(
            flat.first(())
                .expect("remaining flat member")
                .remove()
                .index(),
            index
        );
    }
    assert!(compact.is_empty(()));
    assert!(flat.is_empty(()));
    assert!(Row::try_new(&compact, 255).is_err());
    assert!((&compact).find((), 255).is_none());
}

#[test]
fn wraps_domain() {
    let mut owner = Inline::<256>::default();
    for index in [0, 63, 64, 127, 128, 191, 192, 255] {
        Row::clamp(&owner, index).update::<true>(());
    }
    owner.cursor = 255;
    for expected in [255, 0, 63, 64, 127, 128, 191, 192] {
        let start = usize::from(owner.cursor);
        let removed = (&mut owner).pop_from((), start).expect("cyclic member");
        assert_eq!(removed.index(), expected);
        owner.cursor = (expected as u8).wrapping_add(1);
    }
    assert!(owner.is_empty(()));
}

#[test]
fn initializes_width() {
    for (bits, words) in [
        (0, 0),
        (1, 1),
        (32, 1),
        (33, 3),
        (256, 9),
        (1024, 33),
        (1025, 36),
    ] {
        let shape = bitmap::Shape::<u32, u32>::new(bits);
        assert_eq!(shape.words(), words);
        let mut owner = Bitmap::try_new(shape, bitmap::Fill::Full).expect("narrow fixture");
        for index in 0..bits as usize {
            assert_eq!(
                owner
                    .first(())
                    .expect("initialized member")
                    .remove()
                    .index(),
                index
            );
        }
        assert!(owner.is_empty(()));
        assert!((&owner).find((), bits as usize).is_none());
        if bits != 0 {
            let last = bits as usize - 1;
            Row::try_new(&owner, last)
                .expect("last row")
                .update::<true>(());
            assert_eq!(
                (&owner).find((), 0).expect("restored summaries").index(),
                last
            );
        }
    }
    assert_eq!(mem::size_of::<bitmap::Word>(), mem::size_of::<usize>());
    assert_eq!(mem::align_of::<bitmap::Word>(), mem::align_of::<usize>());
    assert_eq!(mem::size_of::<bitmap::Word<u32>>(), 4);
    assert_eq!(mem::align_of::<bitmap::Word<u32>>(), 4);
    assert_eq!(
        mem::size_of::<bitmap::Shape<u32, u32>>(),
        mem::size_of::<bitmap::Shape>()
    );
}
