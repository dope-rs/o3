//! Shared storage proofs and operations that consume their exact owning capabilities.

use std::{mem, num::NonZero, slice};

pub use proof::Proof;
pub(in crate::depot) use proof::{ChainCount, Drain, Numeric, Sealed};

/// Initialized membership retaining its exact source, key, and operation predicate.
pub(in crate::depot) type Live<S, K, P = ()> = Proof<(S, K), Initialized<P>>;

/// # Safety
/// The output retains the source's ownership, lifetime, exact slot, state, and plane.
/// Borrowed outputs cannot create ownership or exclusivity absent from the retained source.
pub unsafe trait Project<K, P = ()> {
    type Output;
    /// # Safety
    /// This key names an initialized slot in this exact retained source and plane.
    /// All ownership and state invariants specified by P hold for the retained operation.
    unsafe fn project(self, key: K) -> Self::Output;
}

use crate::depot::{
    arena::linked::{head, slots},
    capacity,
    index::{Initialized, Stored, bitmap, bitmap::entry::Row, set},
    queue::{
        indexed::keyed::{self, lanes},
        links::Index,
        pool::{Lane, fifo::Fifo},
        ring, tagged,
    },
};
mod proof;

impl crate::alloc::raw::Empty<capacity::Count> {
    // SAFETY: Count::EMPTY is zero and its Extent implementation preserves that count.
    pub const EMPTY: Self = unsafe { Self::assume(capacity::Count::EMPTY) };
}

/// # Safety
/// Copies and equal identities retain one direct-address index for life.
/// `index` is total and numeric, returning normally without callbacks or storage reentry.
pub unsafe trait Address: Copy + Eq {
    fn index(self) -> usize;
}

/// # Safety
/// Direct-address keys reject `VACANT` and exactly round-trip live identities and indices.
/// `index` obeys [`Address`]'s projection contract.
pub unsafe trait Key: Copy + Eq {
    type Raw: Copy + Eq;
    type Live: Copy + Eq;
    const VACANT: Self::Raw;
    fn index(self) -> usize;
    fn into_raw(self) -> Self::Raw;
    /// Reconstructs a key from an identity proven emitted by this key domain.
    fn from_raw(raw: Stored<Self>) -> Self;
    /// # Safety
    /// The retained identity is live in this exact `Self` domain with unchanged logical identity.
    /// Implementations return that exact destination identity.
    unsafe fn rebuild<K: Key<Raw = Self::Raw>>(stored: Stored<K>) -> Self {
        // SAFETY: the caller preserves the emitted identity in this destination domain.
        Self::from_raw(unsafe { Stored::new(stored.into_raw()) })
    }
    fn decode(raw: Self::Raw) -> Option<Self>;
    fn into_live(self) -> Self::Live;
    fn from_live(live: Self::Live) -> Self;
}

/// Stored representation of one external forward link.
/// # Safety
/// `NONE` decodes to `None`; present links and live items round-trip exactly.
pub unsafe trait Link: Copy + Eq {
    type Item: Copy + Eq;
    const NONE: Self;
    fn item(self) -> Option<Self::Item>;
    /// Stores one live item without changing its identity.
    /// # Safety
    /// `item` must be live in this link's exact domain.
    unsafe fn some(item: Self::Item) -> Self;
}

/// # Safety
/// The retained owner initializes this key and satisfies every ownership and state invariant of P.
/// Shared source loans preserve those obligations without creating exclusive authority.
pub unsafe trait Source<P = ()>: Sized {
    type Key: Copy;
    fn key(&self) -> Self::Key;
}

/// # Safety
/// Dense ordinal values bijectively map to `0..=MAX`; conversions are inverse and `ZERO` maps to zero.
/// In-domain conversions return normally without callbacks or storage reentry.
pub unsafe trait Word: Copy + Eq {
    const ZERO: Self;
    const MAX: NonZero<usize>;
    fn offset(self) -> usize;
    /// # Safety
    /// `offset` must not exceed [`Self::MAX`].
    unsafe fn from_offset(offset: usize) -> Self;
}

pub(in crate::depot) use hooks::{
    Detach, EndpointHook, EndpointWord, Insert, Membership, Neighbor, Pop, Relink, Removal, Splice,
};
pub(crate) use proof::{Present, Slot};
mod arena;
pub use arena::Arena;
mod words;
pub use words::Words;
/// Unsafe projections that preserve a retained bitmap row.
pub trait Rows<W: bitmap::Width = usize>: Words<W> + Sized {
    /// # Safety
    /// Mutation preserves this source's storage, immutable domain, row, and phase membership.
    /// Implementations must return this row's exact retained source.
    unsafe fn source_mut<P, M>(row: &mut Row<Self, P, M, W>) -> &mut Self {
        &mut row.source
    }

    /// # Safety
    /// Projection retains the exact source, immutable domain, and owner lifetime.
    /// Implementations must retain this row's exact index and projected owner.
    unsafe fn map_row<T: Words<W>>(
        row: Row<Self, (), (), W>,
        map: impl FnOnce(Self) -> T,
    ) -> Row<T, (), (), W> {
        unsafe { row.map(map) }
    }
}

impl<W: bitmap::Width, S: Words<W>> Rows<W> for S {}
mod hooks;
pub use hooks::Hooks;

/// An immutable u32 count retaining its original domain proof.
/// # Safety
/// Every copy reports the same count within MIN..=MAX; count is zero or count - 1 is at most I::MAX.
pub unsafe trait Count<I: Compact = u32>: Copy {
    const MIN: u32 = 0;
    const MAX: u32 = u32::MAX;
    fn count(self) -> capacity::Count;
}

/// An exact input traversal retaining its terminal operation state.
/// # Safety
/// The remaining count equals future items and never exceeds u32::MAX.
pub unsafe trait Exact: Iterator {
    type Output;
    fn remaining(&self) -> capacity::Count;
    /// # Safety
    /// Iteration reached its terminal None without an intervening mutation.
    unsafe fn finish(self) -> Self::Output;
}

/// Unchanged inputs bound to their exact slot demand.
/// # Safety
/// The reported count belongs to this unchanged input until it starts or is rejected.
pub unsafe trait Input<T>: Sized {
    type Item;
    type Error;
    type Context;
    type Output;
    fn count(&self) -> capacity::Count;
    fn reject(self, error: capacity::Shortage) -> Self::Error;
}

/// An input whose start needs no further admission.
/// # Safety
/// Starting preserves the count, original owner, operation, and all lifetimes.
pub unsafe trait Start<T>: Input<T> {
    fn start(
        self,
    ) -> (
        impl Exact<Item = Self::Item, Output = Self::Output>,
        Self::Context,
    );
}

/// An input whose start can reject additional resource admission.
/// # Safety
/// Success preserves the input proof; rejection leaves the reserved slots unused.
pub unsafe trait TryStart<T>: Input<T> {
    fn try_start(self) -> Started<Self, T, impl Exact<Item = Self::Item, Output = Self::Output>>;
}

/// Native positions in the fixed domain `0..N`.
/// # Safety
/// Values must identify positions below `N` bijectively; reconstruction is inverse.
pub unsafe trait BitIndex<const N: usize>: Copy {
    fn index(self) -> usize;
    /// # Safety
    /// The position must be below `N`, and `N` must be at most sixteen.
    unsafe fn from_index(index: usize) -> Self;
}

/// A retained capability for one stationary planar owner.
/// # Safety
/// Projections are pure; every projection and move retains the same live owner, extent, and lifetime.
pub unsafe trait SetSource<Tag, Raw, const N: usize> {
    fn planes(&self) -> &set::State<Tag, Raw, N>;
}

/// A storage owner whose exclusive borrow excludes every other plane access.
/// # Safety
/// No independent handle may access these words while this owner is exclusively borrowed.
pub unsafe trait Bitmap<W: bitmap::Width = usize>: Words<W> {}

/// # Safety
/// Stable disjoint domains retain their heads; tagged operations own mutations and `&mut Self` excludes competing access.
/// Projections return normally without reentering the topology.
pub unsafe trait TagStore {
    type Head: tagged::raw::Head;
    type Heads: AsRef<[Self::Head]> + ?Sized;
    fn heads(&self) -> &Self::Heads;
    fn capacity(&self) -> usize;
    /// # Safety
    /// The index names an initialized node in this exact owner.
    unsafe fn hook(&self, index: Index) -> &tagged::Hook;
}
/// An entry whose destination belongs to its own store.
/// # Safety
/// The lane belongs to TagEntry::store; its projection returns normally without topology reentry or mutation.
pub unsafe trait TagTarget: TagEntry {
    fn lane(&self) -> tagged::Endpoint<&Self::Store>;
}
/// # Safety
/// Storage owns the exact entry and completes its phase once on drop; projections preserve it.
/// into_inner disarms completion and transfers that entry with its owner and membership unchanged.
pub unsafe trait TagFinish<E: TagEntry> {
    type Storage;

    fn new(entry: E) -> Self::Storage;
    fn entry(storage: &Self::Storage) -> &E;
    fn entry_mut(storage: &mut Self::Storage) -> &mut E;
    fn into_inner(storage: Self::Storage) -> E;
}
mod tagentry;
pub use tagentry::TagEntry;

/// The exact backing and initialized logical range of one ring.
/// # Safety
/// State and backing agree: capacity fits Word, head is bounded, and `0..len` is initialized.
pub unsafe trait RingStore {
    type Item;
    type Word: Word;
    fn state(&self) -> ring::State<Self::Word>;
    fn capacity(&self) -> usize;
    fn entries(&self) -> *const mem::MaybeUninit<Self::Item>;
}
/// Exclusive access to the state and disjoint backing of one ring.
/// # Safety
/// Projection returns normally with this owner's exact state and complete backing of length RingStore::capacity.
pub unsafe trait RingEdit: crate::depot::raw::RingStore {
    /// Lends the two initialized runs under one exclusive owner loan.
    /// # Safety
    /// Preserve owner invariants after mutation, unless terminal ownership prevents reuse.
    unsafe fn slices_mut(&mut self) -> (&mut [Self::Item], &mut [Self::Item]) {
        // SAFETY: RingEdit retains the exact initialized logical range;
        // the physical runs are disjoint under the complete exclusive owner loan.
        unsafe {
            let (state, entries) = self.parts_mut();
            let (start, [front, back]) = state.prefix(entries.len(), state.len());
            let entries = entries.as_mut_ptr().cast::<Self::Item>();
            (
                slice::from_raw_parts_mut(entries.add(start), front),
                slice::from_raw_parts_mut(entries, back),
            )
        }
    }

    fn push<const FRONT: bool>(vacant: ring::Vacant<'_, Self>, value: Self::Item) {
        // SAFETY: consuming vacancy retains the complete exclusive owner and one spare slot.
        unsafe {
            let (state, entries) = vacant.into_source().parts_mut();
            let index = state.push::<FRONT>(entries.len());
            (*entries.as_mut_ptr().add(index)).write(value);
        }
    }

    /// # Safety
    /// Preserve state, initialization, and ownership invariants unless terminal ownership prevents reuse.
    unsafe fn parts_mut(
        &mut self,
    ) -> (
        &mut ring::State<Self::Word>,
        &mut [mem::MaybeUninit<Self::Item>],
    );

    /// # Safety
    /// The caller retains an exclusive nonempty owner and must preserve its metadata.
    /// Implementations must retain this exact owner and its initialized front.
    unsafe fn front_raw(&mut self) -> ring::Front<'_, Self> {
        unsafe { ring::Front::new(self) }
    }

    /// # Safety
    /// The exclusive owner has spare capacity; the caller must preserve its metadata.
    /// Implementations must retain this exact owner and its vacancy.
    unsafe fn vacant_raw(&mut self) -> ring::Vacant<'_, Self> {
        unsafe { ring::Vacant::new(self) }
    }
}
mod ringcell;
pub use ringcell::RingCell;
pub(crate) use ringcell::{RingDestroy, RingRead, drop_ring};

/// Detaches one exact external head slot.
/// # Safety
/// Returns that item's stored successor after detaching the supplied live item.
pub unsafe trait HeadPop<L: Link, I = ()>: Sized {
    type Output;
    /// # Safety
    /// `item` is the exact live slot owned by this transition.
    unsafe fn pop(self, item: L::Item, input: I) -> (L, Self::Output);
    /// # Safety
    /// This transition owns this exact current front and cannot reenter its head.
    unsafe fn apply<O: head::Owner<L>>(self, front: head::Front<O, L>, input: I) -> Self::Output {
        unsafe { front.pop(self, input) }
    }
}

/// Publishes one exact detached external slot.
/// # Safety
/// Stores `head` in that detached slot before returning its exact live identity.
pub unsafe trait HeadPush<L: Link, I = ()>: Sized {
    /// # Safety
    /// This transition and `input` own the exact detached slot returned.
    unsafe fn push(self, input: I, head: L) -> L::Item;
    /// # Safety
    /// This transition owns a detached slot in this exact head's storage and cannot reenter it.
    unsafe fn apply<O: head::Owner<L>>(self, head: O, input: I) {
        let item = unsafe { self.push(input, head.raw()) };
        head.store(unsafe { L::some(item) });
    }
}

/// Consumes one exact keyed slot into its next membership phase.
/// # Safety
/// Output preserves or consumes this slot's original owner and root authority.
pub unsafe trait KeyFinish<const LINKED: bool, P = ()>: Sized {
    type Output;
    /// # Safety
    /// The selected slot is a member when LINKED and canonically detached otherwise.
    unsafe fn finish(self) -> Self::Output;
}
/// # Safety
/// This exclusive root retains its exact initialized slot, key, link and FIFO.
/// All safe borrows and escaping values preserve retained slot/link authority and membership; indices exclude `Fifo` endpoint tags.
pub unsafe trait KeyOwner<P = ()>: Sized {
    type Key: Key;
    type Fifo;
    fn parts(&self) -> (Self::Key, &keyed::Link<Self::Key, P>);
    fn slot(&self) -> keyed::Entry<'_, Self::Key, P> {
        keyed::Entry::from_source(self)
    }
    fn fifo(&self) -> &Self::Fifo;
    fn entry<'a>(slot: Proof<(&'a Self, Index), Initialized<P>>) -> keyed::Entry<'a, Self::Key, P>;
}
/// # Safety
/// Consumption retains the exact key/link pair (index below `u32::MAX`), initialized storage and plane for `'a`.
pub unsafe trait KeyBorrow<'a, P = ()> {
    type Key: Key;
    fn parts(self) -> (Self::Key, &'a keyed::Link<Self::Key, P>);
}
// SAFETY: the shared loan retains the owner's exact selected key and link plane.
unsafe impl<'a, O: KeyOwner<P>, P> KeyBorrow<'a, P> for &'a O {
    type Key = O::Key;
    fn parts(self) -> (Self::Key, &'a keyed::Link<Self::Key, P>) {
        O::parts(self)
    }
}
// SAFETY: Entry already retains this exact initialized key and co-located link.
unsafe impl<'a, K: Key, P> KeyBorrow<'a, P> for keyed::Entry<'a, K, P> {
    type Key = K;
    fn parts(self) -> (K, &'a keyed::Link<K, P>) {
        (self.key(), self.link())
    }
}
/// # Safety
/// Routing preserves this exact live slot, vacancy and exclusive root in its destination FIFO.
pub unsafe trait KeyRoute<P = ()>: KeyOwner<P> {
    type Target: KeyOwner<P, Key = Self::Key, Fifo = Self::Fifo>;
    /// # Safety
    /// This slot must be detached before changing its eligible FIFO.
    unsafe fn route(self) -> Self::Target;
}
/// # Safety
/// Retaining this source preserves its paired initialized storage, live endpoints and link plane.
pub unsafe trait KeySource<P = ()> {
    type Key: Key;
    fn fifo(&self) -> &keyed::Fifo<Self::Key, P>;
}
/// # Safety
/// Shared traversal preserves membership, links and identities; projection resolves the exact live slot.
pub unsafe trait KeyRead<P = ()>: KeySource<P> {
    fn entry<'a>(live: Proof<(&'a Self, Index), Initialized<P>>) -> keyed::Entry<'a, Self::Key, P>;
}
/// # Safety
/// Every FIFO edge is live in this exact owner; selection retains its exclusive authority.
pub unsafe trait KeyPlane<P = ()>:
    KeySource<P, Key = <Self::Slot as KeyOwner<P>>::Key> + Sized
{
    type Slot: KeyOwner<P, Fifo = keyed::Fifo<Self::Key, P>>;
    fn select(slot: Proof<(Self, Index), Initialized<P>>) -> Self::Slot;
}
/// # Safety
/// Both disjoint FIFOs retain their exact initialized storage, plane and membership exclusively.
pub unsafe trait KeyJoin<P = ()>: KeySource<P> {
    fn target(&self) -> &keyed::Fifo<Self::Key, P>;
    fn link(live: Proof<(&Self, Index), Initialized<P>>) -> &keyed::Link<Self::Key, P>;
}
/// # Safety
/// This exclusive owner retains its heads/links with slots below `u32::MAX + 1 - N`.
/// Shared aliases and payload operations preserve topology, live slots and exclusive authority.
pub unsafe trait LaneSelect<const N: usize, const L: usize, P = ()>:
    Sized + Project<Index, P> + AsRef<lanes::Fifo<Self::Key, N, P>>
{
    type Key: Key;
    type Slot: KeyOwner<P, Key = Self::Key, Fifo = lanes::Fifo<Self::Key, N, P>>
        + From<Self::Output>;
}
/// # Safety
/// Encodings through `MAX` are complete, injective, and round-trip exactly.
/// Both conversions are deterministic, non-panicking, and never invoke storage operations using this index domain.
pub unsafe trait Compact: Copy {
    const MAX: u32;
    fn into_raw(self) -> u32;
    /// Reconstructs an identity previously encoded by this domain.
    /// # Safety
    /// `raw <= Self::MAX` and belongs to this index domain.
    unsafe fn from_raw(raw: u32) -> Self;
}

/// # Safety
/// Nodes retain the exact topology owner; count bounds peak claims, including recycled slots.
pub unsafe trait LinkedBatch<'a, T: 'a, Tag: 'a>: Sized {
    type Output;
    type Error;
    fn nodes(&self) -> &'a Arena<T, Tag>;
    fn count(&self) -> capacity::Count;
    fn reject(self, error: capacity::Shortage) -> Self::Error;
    fn run(self, reserve: &mut slots::Reserve<'a, T, Tag>) -> Self::Output;
}

/// An owned insertion carrying its original lane and unary slot entitlement.
///
/// # Safety
/// The item owns this allocation's unused unary credit until its slot is removed.
pub unsafe trait PoolInsert<'a, const TRACK: bool = true>: Sized {
    type Item: 'a;
    type State: 'a;
    /// # Safety
    /// The returned lane and item must be consumed by the same insertion.
    unsafe fn into_parts(self) -> (Lane<'a, Self::Item, Self::State, TRACK>, Self::Item);
}
/// # Safety
/// The credit reserves one unused slot in this exact FIFO's arena and remains
/// paired with the returned entry until insertion or cancellation.
pub unsafe trait Reserve<'fifo, 'pool: 'fifo, const TRACK: bool = true>: Sized {
    type Item: 'pool;
    type Credit;
    type Reserved;

    /// # Safety
    /// Both returned owners must be consumed by this same reservation.
    unsafe fn into_parts(self) -> (&'fifo mut Fifo<'pool, Self::Item, TRACK>, Self::Credit);

    /// # Safety
    /// Inputs come from this same reservation; retain them together through publication or cancellation.
    /// The returned owner releases its vacant slot before returning unused credit on cancellation.
    unsafe fn build(
        slot: slots::Vacant<Self::Item, &'fifo mut Fifo<'pool, Self::Item, TRACK>>,
        credit: Self::Credit,
    ) -> Self::Reserved;
}
pub(crate) use arena::{EndsPop, SlotOwner};
pub(in crate::depot) use arena::{
    IndexedBlock, IndexedHook, IndexedMeta, IndexedState, PoolMutable, PoolOwner, PoolPolicy,
    PoolSharing, QueueBlock, QueueSlot, Retired, SlabCapacity, SlabLinks, SlabSlot, StagedMove,
    StagedNode, StagedPhase, StagedState,
};
pub(in crate::depot) use hooks::{LaneInit, LaneStore};

mod entry;
mod links;
pub use entry::Entry;
pub use links::Links;

/// Start admission returns the unchanged input context or its exact rejection.
type Started<S, T, I> = Result<(I, <S as Input<T>>::Context), <S as Input<T>>::Error>;

/// Consumes a retained interior proof that already excludes live borrows.
///
/// The predicate carries the exclusivity obligation, so interior admission
/// needs the same single check the predicate already performed at its source.
pub trait Admit {
    type Output;

    fn admit(self) -> Self::Output;
}
