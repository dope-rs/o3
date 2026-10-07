use std::{mem::size_of, num::NonZero};

use o3::base::{
    marker::ThreadBound,
    num::{
        Epoch, Generation,
        bounded::{Error, NonZeroU32, NonZeroUsize, Overflow, U32, U64, Usize},
    },
};

#[test]
fn u32_bounds() {
    assert!((U32::<1, 65_535>::new(0)).is_none());
    assert_eq!(U32::<1, 65_535>::new(1).map(U32::get), Some(1));
    assert_eq!(U32::<1, 65_535>::new(65_535).map(U32::get), Some(65_535));
    assert!((U32::<1, 65_535>::new(65_536)).is_none());
    assert_eq!(
        U32::<0, { u32::MAX }>::from_usize(u32::MAX as usize).map(U32::get),
        Some(u32::MAX)
    );
    assert!((U32::<0, { u32::MAX }>::from_usize(u32::MAX as usize + 1)).is_none());
    assert!((U32::<2, 1>::new(1)).is_none());
    assert!((U32::<2, 1>::new(2)).is_none());
    assert_eq!(size_of::<U32<1, 31>>(), size_of::<u32>());
}

#[test]
fn nonzero_u32_bounds() {
    const MIN: NonZeroU32<3, 7> = NonZeroU32::exact::<3>();
    assert_eq!(NonZeroU32::new(3), Some(MIN));
    assert_eq!(
        [0, 2, 7, 8].map(|value| NonZeroU32::<3, 7>::new(value).map(NonZeroU32::get)),
        [None, None, Some(7), None]
    );
    let max = NonZeroU32::<3, 7>::from_usize(7).expect("inclusive upper bound");
    assert_eq!(max.nonzero().get(), 7);
    assert_eq!(max.into_u32().get(), 7);
    assert_eq!(max.into_usize(), 7);
    assert!(NonZeroU32::<1, { u32::MAX }>::from_usize(usize::MAX).is_none());
}

#[test]
fn nonzero_usize_bounds() {
    assert_eq!(
        [0, 2, 3, 7, 8].map(|value| NonZeroUsize::<3, 7>::new(value).map(NonZeroUsize::get)),
        [None, None, Some(3), Some(7), None]
    );
    let bound = NonZeroUsize::<3, 7>::new(3).expect("inclusive lower bound");
    assert_eq!(bound.into_count().get(), 3);
    assert_eq!(bound.prior().get(), 2);
}

#[test]
fn count_preserves_bounds() {
    let one = NonZeroUsize::<1, 1>::new(1).expect("single capacity");
    assert_eq!(one.after(Usize::ZERO), Some(one));
    assert_eq!(one.after(one.into_count()), None);
    let max = NonZeroUsize::<1, { usize::MAX }>::new(usize::MAX).expect("full capacity");
    assert_eq!(max.after(max.prior()), Some(max));
    assert_eq!(max.after(max.into_count()), None);
    const EMPTY: Option<NonZeroUsize<1, 0>> = Usize::<0>::ZERO.nonzero();
    assert!(EMPTY.is_none());
    let one = Usize::<1>::ZERO.try_add(1).expect("single capacity");
    assert_eq!(one.nonzero().map(NonZeroUsize::get), Some(1));
    let full = Usize::<{ usize::MAX }>::ZERO
        .try_add(usize::MAX)
        .expect("full capacity");
    assert_eq!(full.nonzero().map(NonZeroUsize::get), Some(usize::MAX));
    assert_eq!(
        full.try_add(1),
        Err(Error {
            value: usize::MAX,
            incoming: NonZero::<usize>::MIN,
            maximum: usize::MAX,
        })
    );
}

#[test]
fn one_based_retains_bounds() {
    assert_eq!(U32::from_one(NonZero::<u32>::MIN).into_usize(), 0);
    assert_eq!(
        U32::from_one(NonZero::<u32>::MAX).into_usize(),
        u32::MAX as usize - 1
    );
    const ZERO: U32<0, 0> = NonZeroU32::<1, 1>::exact::<1>().into_index();
    const LAST: U32<0, { u32::MAX - 1 }> =
        NonZeroU32::<1, { u32::MAX }>::exact::<{ u32::MAX }>().into_index();
    assert_eq!((ZERO.get(), LAST.get()), (0, u32::MAX - 1));
    assert_eq!(NonZeroU32::<3, 7>::exact::<3>().into_index::<6>().get(), 2);
}

#[test]
fn u32_exact_overflow() {
    const ZERO: Result<U32<0, 0>, Overflow<0>> = U32::try_usize(0);
    assert_eq!(ZERO.expect("zero belongs to the zero domain").get(), 0);
    assert!(matches!(U32::<0, 0>::try_usize(1), Err(error) if error.actual() == 1));
    assert_eq!(U32::<0, 7>::try_usize(7).expect("inclusive limit").get(), 7);
    assert!(matches!(U32::<0, 7>::try_usize(8), Err(error) if error.actual() == 8));
    assert!(
        matches!(U32::<0, 7>::try_usize(usize::MAX), Err(error) if error.actual() == usize::MAX)
    );
    assert_eq!(
        U32::<0, { u32::MAX }>::try_usize(u32::MAX as usize)
            .expect("full u32 domain")
            .get(),
        u32::MAX
    );
    assert!(
        matches!(U32::<0, { u32::MAX }>::try_usize(usize::MAX), Err(error) if error.actual() == usize::MAX)
    );
}

#[test]
fn u32_clamps() {
    assert_eq!(U32::<3, 7>::clamp(2).get(), 3);
    assert_eq!(U32::<3, 7>::clamp(5).get(), 5);
    assert_eq!(U32::<3, 7>::clamp(8).get(), 7);
    assert_eq!(U32::<3, 7>::new(5).map(U32::into_usize), Some(5));
}

#[test]
fn u32_reuses_bounds() {
    const LOW: U32<0, 9> = U32::<3, 7>::exact::<3>().widen();
    const HIGH: U32<0, 9> = U32::<3, 7>::exact::<7>().widen();
    const SAME: U32<3, 7> = U32::<3, 7>::exact::<7>().widen();
    assert_eq!((LOW.get(), HIGH.get(), SAME.get()), (3, 7, 7));
}

#[test]
fn u32_derived_bounds() {
    const LOW: U32<3, 7> = U32::exact::<3>();
    const HIGH: U32<3, 7> = U32::exact::<7>();
    const DIFFERENCE: U32<0, 7> = LOW.abs_diff(HIGH);
    assert_eq!(
        (
            DIFFERENCE.get(),
            HIGH.abs_diff(LOW).get(),
            LOW.abs_diff(LOW).get()
        ),
        (4, 4, 0)
    );
    assert_eq!(
        U32::<0, { u32::MAX }>::exact::<0>()
            .abs_diff(U32::exact::<{ u32::MAX }>())
            .get(),
        u32::MAX
    );
    assert_eq!(U32::<0, 7>::prefix(usize::MAX, U32::exact::<0>()).get(), 0);
    assert_eq!(
        [0, 3, usize::MAX].map(|value| U32::<0, 7>::prefix(value, U32::exact::<7>()).get()),
        [0, 3, 7]
    );
}

#[test]
fn u64_keeps_domain_layout() {
    assert_eq!(
        U64::<0, { (1 << 62) - 1 }>::clamp(u64::MAX).get(),
        (1 << 62) - 1
    );
    assert_eq!(U64::<3, 7>::clamp(2).get(), 3);
    assert_eq!(U64::<0, { (1 << 62) - 1 }>::new(0).map(U64::get), Some(0));
    assert_eq!(
        U64::<0, { (1 << 62) - 1 }>::new((1 << 62) - 1).map(U64::get),
        Some((1 << 62) - 1)
    );
    assert!((U64::<0, { (1 << 62) - 1 }>::new(1 << 62)).is_none());
    assert_eq!(
        U64::<0, { (1 << 62) - 1 }>::from_usize(63).map(U64::get),
        Some(63)
    );
    assert!((U64::<2, 1>::new(1)).is_none());
    assert_eq!(
        [0, 0b1100, u64::MAX].map(|value| U64::<0, 0b1010>::mask(value).get()),
        [0, 0b1000, 0b1010]
    );
    assert_eq!(
        (
            U64::<0, 0>::mask(u64::MAX).get(),
            U64::<0, { u64::MAX }>::mask(u64::MAX).get()
        ),
        (0, u64::MAX)
    );
    assert_eq!(size_of::<U64<0, { (1 << 62) - 1 }>>(), size_of::<u64>());
}

#[test]
fn u64_subtraction_preserves_bounds() {
    const BOUND: U64<0, 7> = U64::mask(7);
    const BEFORE: Option<U64<0, 7>> = BOUND.checked_sub(1);
    assert_eq!(BEFORE.map(U64::get), Some(6));
    assert_eq!(
        [0, 7, 8].map(|value| BOUND.checked_sub(value).map(U64::get)),
        [Some(7), Some(0), None]
    );
    assert_eq!(U64::<0, 0>::mask(0).checked_sub(0).map(U64::get), Some(0));
    assert_eq!(
        U64::<0, { u64::MAX }>::mask(u64::MAX)
            .checked_sub(0)
            .map(U64::get),
        Some(u64::MAX)
    );
}

#[test]
fn stops_at_last_identity() {
    let before = Generation::from_raw(u32::MAX - 1);
    let last = (before.next()).expect("the final generation must remain representable");
    assert_eq!(last.get(), u32::MAX);
    assert!((last.next()).is_none());
    assert!((last.checked_add(0)) == (Some(last)));
    assert!((last.checked_add(u32::MAX)).is_none());
    assert!((Generation::<0>::INITIAL.next()).is_none());
    let before = Epoch::new(
        NonZero::new(u64::MAX - 1).expect("nonzero identity"),
        ThreadBound::NEW,
    );
    let last = before.checked_add(1).expect("final epoch");
    assert_eq!(last.get().get(), u64::MAX);
    assert!(last.checked_add(1).is_none());
}
