use o3::{
    ThreadBound,
    buffer::{
        BLOCK_CAPACITY, CapacityError, Cursor, FixedPoolCapacity, Owned, Retained, Shared,
        SharedStr, SpareWriter, Uninitialized,
        queue::Ring,
        view::{Snapshot, window::Inline},
    },
    cell::{Brand, BrandToken, Checked},
    collections::{
        FixedPinSlab, FixedPinSlabVacantEntry, PinSlab, PinSlabVacantEntry, Slab, SlabGeneration,
        SlabKey, SlabKeyParts,
        arena::{Linked, Stack},
        fixed::{hash::Map, index::Slots},
        heap::Min,
        queue::round::Robin,
    },
    mem::{
        budget::{Bytes, Handle, Lease},
        fair::{Credits, Lane, Pool, State},
    },
};

use crate::confined::assert_confined;

assert_confined!(o3::collections::queue::fixed::Fifo<u8>);
assert_confined!(o3::collections::queue::cell::Fifo<u8>);
assert_confined!(o3::collections::queue::slot::CellFifo<u8>);
assert_confined!(o3::collections::queue::slot::Fifo<u8>);
assert_confined!(Robin);
assert_confined!(Linked<u8>);
assert_confined!(Min<u8>);
assert_confined!(Map<u8>);
assert_confined!(PinSlab<u8>);
assert_confined!(PinSlabVacantEntry<'static, u8>);
assert_confined!(FixedPinSlab<u8, 4>);
assert_confined!(FixedPinSlabVacantEntry<'static, u8, 4>);
assert_confined!(Slab<u8>);
assert_confined!(SlabGeneration);
assert_confined!(SlabKey);
assert_confined!(SlabKeyParts);
assert_confined!(Owned);
assert_confined!(Owned<BLOCK_CAPACITY>);
assert_confined!(SpareWriter<'static>);
assert_confined!(Shared);
assert_confined!(SharedStr);
assert_confined!(o3::buffer::Bytes<Retained>);
assert_confined!(Snapshot<16_384>);
assert_confined!(o3::buffer::Pool);
assert_confined!(o3::buffer::Lease);
assert_confined!(o3::buffer::Pool<Uninitialized, FixedPoolCapacity<BLOCK_CAPACITY>>);
assert_confined!(Cursor<FixedPoolCapacity<BLOCK_CAPACITY>>);
assert_confined!(Inline<64>);
assert_confined!(Ring);
assert_confined!(Bytes);
assert_confined!(Handle<'static>);
assert_confined!(Lease<'static>);
assert_confined!(Credits);
assert_confined!(Pool);
assert_confined!(Lane<'static>);
assert_confined!(ThreadBound);
assert_confined!(BrandToken<'static>);
assert_confined!(Brand<'static, u8>);
assert_confined!(Checked<u8>);

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CapacityError>();
    assert_send_sync::<Slots<u8>>();
    assert_send_sync::<Stack<u8>>();
};

const _: fn() = || {
    trait AmbiguousIfUnpin<A> {}
    impl<T: ?Sized> AmbiguousIfUnpin<()> for T {}
    impl<T: ?Sized + Unpin> AmbiguousIfUnpin<u8> for T {}

    fn not_unpin<T: ?Sized + AmbiguousIfUnpin<A>, A>() {}
    not_unpin::<FixedPinSlab<u8, 4>, _>();
};

#[test]
fn state_is_confined_and_keys_are_word_sized() {
    assert_eq!(std::mem::size_of::<ThreadBound>(), 0);
    assert_eq!(std::mem::size_of::<BrandToken<'static>>(), 0);
    assert_eq!(std::mem::size_of::<SlabKey>(), 8);
    assert_eq!(std::mem::size_of::<SlabKeyParts>(), 8);
    assert_eq!(std::mem::size_of::<SlabGeneration>(), 4);
    assert_eq!(std::mem::size_of::<Linked<u8>>(), 48);
    assert_eq!(std::mem::size_of::<Stack<u8>>(), 48);
    assert_eq!(std::mem::size_of::<Pool>(), 8);
    assert_eq!(std::mem::size_of::<Pool<2>>(), 16);
    assert_eq!(std::mem::size_of::<State>(), 16);
    assert_eq!(std::mem::size_of::<State<2>>(), 32);
    assert_eq!(std::mem::size_of::<Lane<'_>>(), 24);
    assert_eq!(std::mem::size_of::<Lane<'_, 2>>(), 32);
    assert_eq!(std::mem::size_of::<Credits>(), 40);
    assert_eq!(std::mem::size_of::<Credits<2>>(), 64);
    assert_eq!(
        std::mem::size_of::<CapacityError>(),
        std::mem::size_of::<usize>() * 2
    );
}

#[test]
fn checked_cell_rejects_reentry_and_restores_access_after_unwind() {
    let cell = Checked::new(1_u64);
    let reentered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cell.with_mut(|_| cell.with_mut(|_| {}));
    }));
    assert!(reentered.is_err());

    assert_eq!(
        cell.with_mut(|value| {
            *value += 1;
            *value
        }),
        2,
    );
    assert_eq!(std::mem::size_of_val(&cell), std::mem::size_of::<u64>() * 2,);
}
