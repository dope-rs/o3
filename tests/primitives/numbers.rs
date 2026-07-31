use std::mem::size_of;

use o3::num::{BoundedU32, BoundedU64};

#[test]
fn bounded_u32_preserves_only_values_inside_its_domain() {
    type Port = BoundedU32<1, 65_535>;

    assert_eq!(Port::new(0), None);
    assert_eq!(Port::new(1).map(Port::get), Some(1));
    assert_eq!(Port::new(65_535).map(Port::get), Some(65_535));
    assert_eq!(Port::new(65_536), None);
}

#[test]
fn bounded_u32_rejects_unrepresentable_and_empty_ranges() {
    type Full = BoundedU32<0, { u32::MAX }>;
    type Empty = BoundedU32<2, 1>;

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
    assert_eq!(size_of::<BoundedU32<1, 31>>(), size_of::<u32>());
}

#[test]
fn bounded_u64_preserves_its_domain_without_layout_cost() {
    type QuicVarInt = BoundedU64<0, { (1 << 62) - 1 }>;

    assert_eq!(QuicVarInt::new(0).map(QuicVarInt::get), Some(0));
    assert_eq!(
        QuicVarInt::new((1 << 62) - 1).map(QuicVarInt::get),
        Some((1 << 62) - 1)
    );
    assert_eq!(QuicVarInt::new(1 << 62), None);
    assert_eq!(QuicVarInt::from_usize(63).map(QuicVarInt::get), Some(63));
    assert_eq!(BoundedU64::<2, 1>::new(1), None);
    assert_eq!(size_of::<QuicVarInt>(), size_of::<u64>());
}
