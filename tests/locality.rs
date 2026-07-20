use crate::confined::assert_confined;
use o3::buffer::{
    ByteRing, CapacityError, Lease, Owned, Pool, RollingBuffer, Shared, SnapshotBuf, SpareWriter,
    View, Writer,
};
use o3::cell::{BrandCell, BrandToken};
use o3::collections::{CellQueue, FixedQueue, SlotQueue};
use o3::collections::{FixedHashTable, IndexedMinHeap};
use o3::collections::{
    FixedPinSlab, FixedPinSlabOccupiedEntry, PinCellSlab, PinCellSlabOccupiedEntry,
    PinCellSlabVacantEntry, PinSlab, PinSlabOccupiedEntry, Slab, SlabGeneration, SlabKey,
    SlabKeyParts,
};
use o3::marker::ThreadBound;
use o3::mem::Scratch;
use o3::mem::{ByteBudget, ByteBudgetHandle, ByteLease};

assert_confined!(FixedQueue<u8>);
assert_confined!(CellQueue<u8>);
assert_confined!(SlotQueue<u8>);
assert_confined!(IndexedMinHeap<u8>);
assert_confined!(FixedHashTable<u8>);
assert_confined!(PinSlab<u8>);
assert_confined!(PinSlabOccupiedEntry<'static, u8>);
assert_confined!(PinCellSlab<u8>);
assert_confined!(PinCellSlabVacantEntry<'static, u8>);
assert_confined!(PinCellSlabOccupiedEntry<'static, u8>);
assert_confined!(FixedPinSlab<u8, 4>);
assert_confined!(FixedPinSlabOccupiedEntry<'static, u8, 4>);
assert_confined!(Slab<u8>);
assert_confined!(SlabGeneration);
assert_confined!(SlabKey);
assert_confined!(SlabKeyParts);
assert_confined!(Owned);
assert_confined!(Writer<'static>);
assert_confined!(SpareWriter<'static>);
assert_confined!(Shared);
assert_confined!(View<'static>);
assert_confined!(SnapshotBuf<16_384>);
assert_confined!(Pool);
assert_confined!(Lease<'static>);
assert_confined!(RollingBuffer<64>);
assert_confined!(ByteRing);
assert_confined!(ByteBudget);
assert_confined!(ByteBudgetHandle<'static>);
assert_confined!(ByteLease<'static>);
assert_confined!(Scratch<u8>);
assert_confined!(ThreadBound);
assert_confined!(CapacityError);
assert_confined!(BrandToken<'static>);
assert_confined!(BrandCell<'static, u8>);

const _: fn() = || {
    trait AmbiguousIfUnpin<A> {}
    impl<T: ?Sized> AmbiguousIfUnpin<()> for T {}
    impl<T: ?Sized + Unpin> AmbiguousIfUnpin<u8> for T {}

    fn not_unpin<T: ?Sized + AmbiguousIfUnpin<A>, A>() {}
    not_unpin::<FixedPinSlab<u8, 4>, _>();
    not_unpin::<PinCellSlab<u8>, _>();
};

#[test]
fn state_is_confined_and_keys_are_word_sized() {
    assert_eq!(std::mem::size_of::<ThreadBound>(), 0);
    assert_eq!(std::mem::size_of::<BrandToken<'static>>(), 0);
    assert_eq!(std::mem::size_of::<SlabKey>(), 8);
    assert_eq!(std::mem::size_of::<SlabKeyParts>(), 8);
    assert_eq!(std::mem::size_of::<SlabGeneration>(), 4);
    assert_eq!(
        std::mem::size_of::<PinCellSlabVacantEntry<'static, u8>>(),
        16
    );
    assert_eq!(
        std::mem::size_of::<PinCellSlabOccupiedEntry<'static, u8>>(),
        16
    );
    assert_eq!(
        std::mem::size_of::<CapacityError>(),
        std::mem::size_of::<usize>() * 2
    );
}
