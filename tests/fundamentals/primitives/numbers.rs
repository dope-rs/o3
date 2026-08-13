use std::mem::size_of;

use o3::num::bounded::{NonZeroU64, U32, U64};

#[test]
fn bounded_u32_preserves_only_values_inside_its_domain() {
    type Port = U32<1, 65_535>;

    assert_eq!(Port::new(0), None);
    assert_eq!(Port::new(1).map(Port::get), Some(1));
    assert_eq!(Port::new(65_535).map(Port::get), Some(65_535));
    assert_eq!(Port::new(65_536), None);
}

#[test]
fn bounded_u32_rejects_unrepresentable_and_empty_ranges() {
    type Full = U32<0, { u32::MAX }>;
    type Empty = U32<2, 1>;

    assert_eq!(
        Full::from_usize(u32::MAX as usize).map(Full::get),
        Some(u32::MAX)
    );
    assert_eq!(Full::from_usize(u32::MAX as usize + 1), None);
    assert_eq!(Empty::new(1), None);
    assert_eq!(Empty::new(2), None);
}

#[test]
fn bounded_u32_has_the_same_layout_as_u32() {
    assert_eq!(size_of::<U32<1, 31>>(), size_of::<u32>());
}

#[test]
fn bounded_u32_clamps_converts_and_checks_arithmetic() {
    type Window = U32<3, 7>;

    assert_eq!(Window::clamp_from_usize(2).get(), 3);
    assert_eq!(Window::clamp_from_usize(5).get(), 5);
    assert_eq!(Window::clamp_from_usize(8).get(), 7);
    assert_eq!(Window::new(5).unwrap().into_usize(), 5);
    assert_eq!(
        Window::new(5).unwrap().checked_add(2).map(Window::get),
        Some(7)
    );
    assert_eq!(Window::new(5).unwrap().checked_add(3), None);
    assert_eq!(
        Window::new(5).unwrap().checked_sub(2).map(Window::get),
        Some(3)
    );
    assert_eq!(Window::new(5).unwrap().checked_sub(3), None);
    assert_eq!(
        Window::new(5)
            .unwrap()
            .checked_add_usize(2)
            .map(Window::get),
        Some(7)
    );
    assert_eq!(Window::new(5).unwrap().checked_add_usize(usize::MAX), None);
}

#[test]
#[should_panic(expected = "cannot clamp into an empty bounded range")]
fn bounded_u32_cannot_clamp_into_an_empty_range() {
    let _ = U32::<2, 1>::clamp_from_usize(1);
}

#[test]
fn bounded_u64_preserves_its_domain_without_layout_cost() {
    type QuicVarInt = U64<0, { (1 << 62) - 1 }>;

    assert_eq!(QuicVarInt::new(0).map(QuicVarInt::get), Some(0));
    assert_eq!(
        QuicVarInt::new((1 << 62) - 1).map(QuicVarInt::get),
        Some((1 << 62) - 1)
    );
    assert_eq!(QuicVarInt::new(1 << 62), None);
    assert_eq!(QuicVarInt::from_usize(63).map(QuicVarInt::get), Some(63));
    assert_eq!(
        QuicVarInt::new(63)
            .unwrap()
            .checked_sub(63)
            .map(QuicVarInt::get),
        Some(0)
    );
    assert_eq!(QuicVarInt::new(63).unwrap().checked_sub(64), None);
    assert_eq!(U64::<2, 1>::new(1), None);
    assert_eq!(size_of::<QuicVarInt>(), size_of::<u64>());
}

#[test]
fn bounded_nonzero_u64_preserves_bounds_arithmetic_and_niche() {
    type Epoch = NonZeroU64<1, 7>;

    assert_eq!(Epoch::new(0), None);
    assert_eq!(Epoch::new(1).map(Epoch::get), Some(1));
    assert_eq!(Epoch::new(7).map(Epoch::get), Some(7));
    assert_eq!(Epoch::new(8), None);
    assert_eq!(
        Epoch::new(6).unwrap().checked_add(1).map(Epoch::get),
        Some(7)
    );
    assert_eq!(Epoch::new(7).unwrap().checked_add(1), None);
    assert_eq!(Epoch::new(1).unwrap().into_usize(), 1);
    assert_eq!(size_of::<Epoch>(), size_of::<u64>());
    assert_eq!(size_of::<Option<Epoch>>(), size_of::<u64>());
}
