use std::mem;

use o3::depot::array;

#[test]
fn retains_fixed_domain() {
    assert_eq!(mem::size_of::<array::Index<255>>(), 1);
    assert_eq!(mem::size_of::<Option<array::Index<255>>>(), 1);
    for index in 0..255 {
        assert_eq!(
            array::Index::<255>::new(index).expect("bounded").index(),
            index
        );
    }
    assert!(array::Index::<255>::new(255).is_none());
    assert!(array::Index::<0>::new(0).is_none());
    assert!(array::Index::<4>::new(usize::MAX).is_none());
    for value in 0..=u8::MAX {
        assert_eq!(
            array::Index::<4>::mask(value).index(),
            usize::from(value & 3)
        );
    }
    assert_eq!(array::Index::<0>::iter().count(), 0);
    for (offset, index) in array::Index::<255>::iter().rev().enumerate() {
        assert_eq!(index.index(), 254 - offset);
        assert_eq!(index.widen::<255>().index(), index.index());
    }
    let values = [0, 2, 4];
    for end in array::Index::<4>::iter() {
        let mut target = mem::MaybeUninit::uninit();
        let added = end.insert(&values, 9, &mut target);
        assert_eq!(added[end.index()], 9);
        assert!(added.iter().copied().filter(|&value| value != 9).eq(values));
        assert_eq!(end.next::<5>().index(), end.index() + 1);
        assert_eq!(
            end.previous::<3>().map(|index| index.index()),
            end.index().checked_sub(1)
        );
        for limit in 0..=6 {
            let gap = end.partition(&values, |value| *value < limit);
            assert_eq!(
                gap.index(),
                values[..end.index()].partition_point(|value| *value < limit)
            );
        }
    }
    assert_eq!(array::Index::<254>::at::<253>().next::<255>().index(), 254);
    assert_eq!(
        array::Index::<1>::at::<0>().insert(&[], 9_u8, &mut mem::MaybeUninit::uninit()),
        &[9]
    );
    let first = array::Index::from(array::Vacancy::<1>::at::<0>());
    assert_eq!(*first.get(&[7]), 7);
    let mut compact = [0; 255];
    let last = array::Index::from(array::Vacancy::<255>::at::<254>());
    *last.get_mut(&mut compact) = 11;
    assert_eq!(compact[254], 11);
}
