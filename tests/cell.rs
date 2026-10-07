use std::pin::Pin;

use o3::{
    alloc::{Gap, Vacant},
    base::{
        cell::{brand, local, region},
        marker::ThreadBound,
        num::Epoch,
    },
    buffer::{
        BLOCK_CAPACITY,
        bytes::{
            scope,
            window::{Snapshot, reserve},
        },
        hold::ranges,
        pool::{self, Layout, Span, owners, state::Initialized},
        resident,
        storage::{Owned, Shared, Str, provided::fixed},
        write::{Slice, Spare},
    },
    depot::{
        arena::slab::{self, key, storage},
        heap::indexed::Min,
        index,
        map::flat::Table,
        queue::{
            indexed::{fifo, lanes::runtime, planes},
            ring::{self, inline},
            round::Set,
        },
    },
    mem::{
        self, Lease,
        credit::{self, Ledger},
    },
};

trait MaybeSend<A> {}
impl<T: ?Sized> MaybeSend<()> for T {}
impl<T: ?Sized + Send> MaybeSend<u8> for T {}

trait MaybeSync<A> {}
impl<T: ?Sized> MaybeSync<()> for T {}
impl<T: ?Sized + Sync> MaybeSync<u8> for T {}

fn confined<T: ?Sized + MaybeSend<A> + MaybeSync<B>, A, B>() {}

const _: fn() = || {
    confined::<Gap<'static, u8>, _, _>();
    confined::<Vacant<'static, u8>, _, _>();
    confined::<ring::Fifo<u8>, _, _>();
    confined::<ring::Cell<u8>, _, _>();
    confined::<inline::Pool<u8, 1>, _, _>();
    confined::<fifo::List<u8>, _, _>();
    confined::<runtime::Fifo<u8>, _, _>();
    confined::<planes::Set<u8, 1>, _, _>();
    confined::<Set<()>, _, _>();
    confined::<Min<u8>, _, _>();
    confined::<Table<u8>, _, _>();
    confined::<slab::Store<u8>, _, _>();
    confined::<slab::Pinned<u8>, _, _>();
    confined::<
        slab::Pinned<u8, (), { u32::MAX }, false, 1, storage::Inline<1>, storage::Eager>,
        _,
        _,
    >();
    confined::<slab::Pinned<u8, (), { u32::MAX }, false, 1, storage::Inline<1>, storage::Lazy>, _, _>(
    );
    confined::<key::Handle, _, _>();
    confined::<index::Stored<key::Handle>, _, _>();
    confined::<index::Domain<key::Handle, u16>, _, _>();
    confined::<index::Ordinal<key::Handle, u16>, _, _>();
    confined::<scope::raw::Window<'static, 2, 3>, _, _>();
    confined::<Owned, _, _>();
    confined::<Owned<(), BLOCK_CAPACITY>, _, _>();
    confined::<Spare<'static>, _, _>();
    confined::<Slice<'static>, _, _>();
    confined::<Shared, _, _>();
    confined::<resident::Shared<'static>, _, _>();
    confined::<resident::Budget<'static>, _, _>();
    confined::<fixed::Loan<'static, ()>, _, _>();
    confined::<Str, _, _>();
    confined::<Snapshot<16_384>, _, _>();
    confined::<pool::Cursor, _, _>();
    confined::<reserve::Ring<17>, _, _>();
    confined::<Ledger, _, _>();
    confined::<Lease<credit::Return<'static>>, _, _>();
    confined::<mem::raw::Pool, _, _>();
    confined::<mem::raw::State, _, _>();
    confined::<mem::raw::Lane<'static, 'static>, _, _>();
    confined::<mem::raw::Charge<mem::raw::Lane<'static, 'static>>, _, _>();
    confined::<mem::raw::Grant<mem::raw::Lane<'static, 'static>>, _, _>();
    confined::<mem::raw::Count, _, _>();
    confined::<mem::raw::Occupied<'static, ()>, _, _>();
    confined::<mem::raw::Vacant<'static, ()>, _, _>();
    confined::<mem::slot::Storage<()>, _, _>();
    confined::<mem::slot::Vacant<'static, ()>, _, _>();
    confined::<mem::slot::Occupied<'static, ()>, _, _>();
    confined::<ranges::Set<'static, 'static, [u8]>, _, _>();
    confined::<ThreadBound, _, _>();
    confined::<Epoch, _, _>();
    confined::<brand::Token<brand::Identity<'static>>, _, _>();
    confined::<brand::Value<brand::Identity<'static>, u8>, _, _>();
    confined::<region::Token<region::Identity<'static>>, _, _>();
    confined::<region::Proof<region::Identity<'static>>, _, _>();
    confined::<region::Value<region::Identity<'static>, u8>, _, _>();
    confined::<region::Token<region::Identity<'static, 1>>, _, _>();
    confined::<region::Value<region::Identity<'static, 1>, u8>, _, _>();

    confined::<pool::Pool, _, _>();
    confined::<Span, _, _>();
    confined::<pool::Pool<Initialized>, _, _>();
    confined::<Span<owners::Plain<Layout>, pool::Owned, Initialized>, _, _>();
    confined::<Span<owners::Plain<Layout>, pool::Owned, pool::Frozen>, _, _>();
};

#[test]
fn keeps_marker_layout() {
    assert_eq!(std::mem::size_of::<ThreadBound>(), 0);
    assert_eq!(std::mem::align_of::<ThreadBound>(), 1);
}

#[test]
fn borrows_branded_regions() {
    brand::Token::scope_pinned(Pin::new(&mut ()), |mut token, (mut io, mut state), _| {
        let value = token.bind(1);
        *token.pin_mut(Pin::new(&value)).get_mut() = 2;
        assert_eq!(*token.pin_mut(Pin::new(&value)).get_mut(), 2);
        let buffer = io.bind(1_u64);
        let counter = state.bind(2_u64);
        let count = state.borrow_mut(&counter);
        let bytes = io.borrow_mut(&buffer);
        *bytes += *count;
        *count += *bytes;
        assert_eq!((*io.borrow(&buffer), *state.borrow(&counter)), (3, 5));
        buffer.write(local::Access::new(&mut io), |value| *value += 2);
        let mut access = local::Access::new(&mut state);
        counter.write(access.reborrow(), |value| *value += 3);
        assert_eq!(counter.read(access.read(), |value| *value), 8);
        access.with_region(|token| *token.borrow_mut(&counter) += 1);
        assert_eq!(*io.borrow(&buffer), 5);
        assert_eq!(*state.borrow(&counter), 9);
    });
}
