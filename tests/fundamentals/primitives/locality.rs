use o3::{
    ThreadBound,
    buffer::{
        self, BLOCK_CAPACITY, CapacityError,
        bytes::Retained,
        pool,
        queue::Ring,
        storage::{Owned, Shared, strings::Str},
        view::{Snapshot, window::Inline},
        write::SpareWriter,
    },
    cell::{Checked, brand, region},
    collections::{
        fixed::{
            arena::{Linked, Stack},
            hash::Map,
            index::Slots,
        },
        heap::Min,
        queue::round::Robin,
        slab,
    },
    mem::{
        budget::{Bytes, Handle, Lease},
        credit::Ledger,
        fair::{Credits, Lane, Pool},
    },
};

use crate::confined::assert_confined;

assert_confined!(o3::collections::queue::fixed::Fifo<u8>);
assert_confined!(o3::queue::Fifo<u8>);
assert_confined!(o3::collections::queue::slot::Cell<u8>);
assert_confined!(o3::collections::queue::slot::Fifo<u8>);
assert_confined!(Robin);
assert_confined!(Linked<u8>);
assert_confined!(Min<u8>);
assert_confined!(Map<u8>);
assert_confined!(slab::pinned::Pool<u8>);
assert_confined!(slab::pinned::VacantEntry<'static, u8>);
assert_confined!(slab::pinned::Fixed<u8, 4>);
assert_confined!(slab::pinned::FixedVacantEntry<'static, u8, 4>);
assert_confined!(slab::Exclusive<u8>);
assert_confined!(slab::key::Generation);
assert_confined!(slab::key::Handle);
assert_confined!(slab::key::Parts);
assert_confined!(Owned);
assert_confined!(Owned<BLOCK_CAPACITY>);
assert_confined!(SpareWriter<'static>);
assert_confined!(Shared);
assert_confined!(Str);
assert_confined!(o3::buffer::bytes::Bytes<Retained>);
assert_confined!(Snapshot<16_384>);
assert_confined!(buffer::Pool);
assert_confined!(buffer::Lease);
assert_confined!(buffer::Pool<pool::state::Uninitialized, pool::FixedCapacity<BLOCK_CAPACITY>>);
assert_confined!(pool::Cursor<pool::FixedCapacity<BLOCK_CAPACITY>>);
assert_confined!(Inline<64>);
assert_confined!(Ring);
assert_confined!(Bytes);
assert_confined!(Handle<'static>);
assert_confined!(Lease<'static>);
assert_confined!(Ledger);
assert_confined!(Credits);
assert_confined!(Pool);
assert_confined!(Lane<'static>);
assert_confined!(ThreadBound);
assert_confined!(brand::Token<'static>);
assert_confined!(brand::Value<'static, u8>);
assert_confined!(region::Token<'static>);
assert_confined!(region::Value<'static, u8>);
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
    not_unpin::<slab::pinned::Fixed<u8, 4>, _>();
};

#[test]
fn state_is_confined_and_keys_are_word_sized() {
    assert_eq!(std::mem::size_of::<ThreadBound>(), 0);
    assert_eq!(std::mem::size_of::<brand::Token<'static>>(), 0);
    assert_eq!(std::mem::size_of::<region::Token<'static>>(), 0);
    assert_eq!(
        std::mem::size_of::<brand::Value<'static, u64>>(),
        std::mem::size_of::<u64>(),
    );
    assert_eq!(
        std::mem::size_of::<region::Value<'static, u64>>(),
        std::mem::size_of::<u64>(),
    );
    assert_eq!(
        std::mem::align_of::<brand::Value<'static, u64>>(),
        std::mem::align_of::<u64>(),
    );
    assert_eq!(
        std::mem::align_of::<region::Value<'static, u64>>(),
        std::mem::align_of::<u64>(),
    );
    assert_eq!(std::mem::size_of::<slab::key::Handle>(), 8);
    assert_eq!(std::mem::size_of::<slab::key::Parts>(), 8);
    assert_eq!(std::mem::size_of::<slab::key::Generation>(), 4);
    assert_eq!(std::mem::size_of::<Linked<u8>>(), 48);
    assert_eq!(std::mem::size_of::<Stack<u8>>(), 48);
    assert_eq!(std::mem::size_of::<Pool>(), 24);
    assert_eq!(std::mem::size_of::<Pool<2>>(), 32);
    assert_eq!(std::mem::size_of::<Lane<'_>>(), 16);
    assert_eq!(std::mem::size_of::<Lane<'_, 2>>(), 16);
    assert_eq!(std::mem::size_of::<Credits>(), 32);
    assert_eq!(std::mem::size_of::<Credits<2>>(), 48);
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
