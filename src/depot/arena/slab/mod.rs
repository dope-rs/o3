use std::{convert, iter, marker, pin::Pin};

pub use cell::Cell;
use cycle::Policy;
pub use slots::Slots;
use storage::Plane as _;

use crate::depot::{
    arena::slab::{self, key::cycle},
    capacity,
    capacity::Physical,
    index::Initialized,
    queue::links,
};

pub mod entries;
pub mod external;
pub mod key;
pub mod lookup;
pub mod storage;

mod cell;
mod entry;
mod slots;
pub use entry::{
    anchor::Anchor, anchored::Anchored, borrowed::Borrowed, occupied::Occupied, rest::Rest,
    restore::Withdrawn, staged::Staged, vacant::Vacant,
};

/// One issued identity retaining its reserved slot until installation or retirement.
pub type Reserved<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, const RECYCLE: bool = false> =
    Vacant<
        crate::mem::Lease<
            entries::Pending<'a, T, MAX, entries::Interior<RECYCLE>>,
            crate::mem::lease::Open,
        >,
        Tag,
    >;

/// Why no identity can currently be reserved from a slab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Vacancy {
    /// Usable slots are occupied or reserved, or the physical capacity is zero.
    Full,
    /// The admitted non-wrapping identity domain is permanently exhausted.
    IdentityExhausted,
}

impl<T, Tag, const MAX: u32, const PARTITIONS: usize> Store<T, Tag, MAX, false, PARTITIONS> {
    /// Reserves every slot and dense index entry transactionally.
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            core: entries::Core::try_new(cap)?,
            tag: marker::PhantomData,
        })
    }
}

/// Slot storage whose initialized values remain pinned at their original addresses.
pub struct Pinned<
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
    S = storage::Heap,
    I = storage::Eager,
> where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) core: entries::Core<T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, S, I>,
    _pin: marker::PhantomPinned,
    tag: marker::PhantomData<fn() -> Tag>,
}

impl<T, Tag, const MAX: u32, const PARTITIONS: usize, I: storage::Init>
    Pinned<T, Tag, MAX, false, PARTITIONS, storage::Heap, I>
{
    /// Allocates one pinned heap extent under its slot initialization policy.
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            core: entries::Core::try_new(cap)?,
            _pin: marker::PhantomPinned,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32, const PARTITIONS: usize, const N: usize, I: storage::Init> Default
    for Pinned<T, Tag, MAX, false, PARTITIONS, storage::Inline<N>, I>
{
    /// Builds the complete inline slot plane without allocating.
    fn default() -> Self {
        Self {
            core: entries::Core::new(),
            _pin: marker::PhantomPinned,
            tag: marker::PhantomData,
        }
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S, I>
    Pinned<T, Tag, MAX, RECYCLE, PARTITIONS, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub fn capacity(&self) -> usize {
        self.core.slots.capacity()
    }

    pub fn is_full(&self) -> bool {
        self.core.is_full()
    }

    pub fn vacant(
        self: Pin<&mut Self>,
    ) -> Option<slab::Anchor<'_, T, Tag, MAX, RECYCLE, PARTITIONS, S, I>> {
        let store = self.into_ref().get_ref();
        Some(slab::Anchor {
            entry: store.core.free()?,
            tag: marker::PhantomData,
        })
    }

    pub fn parts(&self, parts: key::Parts<MAX>) -> Option<&T> {
        Some(entries::Occupied::key(&self.core, parts.index(), parts.generation())?.read())
    }

    pub fn parts_mut(self: Pin<&mut Self>, parts: key::Parts<MAX>) -> Option<Pin<&mut T>> {
        Some(self.anchor(parts)?.into_value())
    }

    pub fn anchor(
        self: Pin<&mut Self>,
        parts: key::Parts<MAX>,
    ) -> Option<slab::Anchored<'_, T, Tag, MAX, RECYCLE, PARTITIONS, S, I>> {
        let store = self.into_ref().get_ref();
        let entry = entries::Occupied::key(&store.core, parts.index(), parts.generation())?;
        Some(slab::Anchored {
            entry: entry.take(),
            tag: marker::PhantomData,
        })
    }

    /// Passes the pinned value to `f`, then drops it in place.
    pub fn remove_with<R>(
        self: Pin<&mut Self>,
        parts: key::Parts<MAX>,
        f: impl FnOnce(u32, Pin<&mut T>) -> R,
    ) -> Option<R> {
        let mut entry = self.anchor(parts)?;
        let result = f(parts.index(), entry.value());
        entry.discard();
        Some(result)
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn capacity(&self) -> usize {
        self.core.slots.capacity()
    }

    /// Nonretired slots, including occupied slots and unpublished reservations.
    pub fn remaining(&self) -> capacity::Count {
        self.core.remaining()
    }

    pub fn len(&self) -> capacity::Count {
        self.core.len()
    }

    pub fn is_full(&self) -> bool {
        self.core.is_full()
    }

    /// Checks temporary or permanent admission without reserving a slot.
    pub fn try_ready(&self) -> Result<(), Vacancy> {
        if self.core.is_full() {
            Err(self.core.refusal())
        } else {
            Ok(())
        }
    }

    pub fn insert(&mut self, value: T) -> Result<key::Handle<Tag, MAX>, T> {
        let Some(pending) = self.core.free() else {
            return Err(value);
        };
        Ok(slab::Vacant::<_, Tag>::from_pending(pending).insert(value))
    }

    /// Reserves a slot or preserves the exact temporary or permanent refusal.
    pub fn try_vacant(
        &mut self,
    ) -> Result<
        slab::Vacant<entries::Pending<'_, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>,
        Vacancy,
    > {
        Ok(slab::Vacant::from_pending(
            self.core.free().ok_or_else(|| self.core.refusal())?,
        ))
    }

    /// Reserves free slots under one exclusive owner loan.
    /// Each yielded reservation owns only its distinct slot.
    pub fn vacancies(
        &mut self,
    ) -> impl Iterator<
        Item = slab::Vacant<
            entries::Pending<'_, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
            Tag,
            false,
        >,
    > {
        let core = &self.core;
        iter::from_fn(move || core.free().map(slab::Vacant::from_pending))
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Borrows occupied values in physical slot order from this exact shared owner.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        (0..self.capacity()).filter_map(|index| {
            entries::Occupied::at(&self.core, index as u32).map(entries::Occupied::read)
        })
    }

    pub fn try_get(
        &self,
        key: key::Handle<Tag, MAX>,
    ) -> Result<
        &T,
        key::Error<Tag, MAX, convert::Infallible, <cycle::Mode<RECYCLE> as Policy>::Retired>,
    > {
        Ok(entries::Occupied::try_key(&self.core, key)?.read())
    }

    pub fn get_mut(&mut self, key: key::Handle<Tag, MAX>) -> Option<&mut T> {
        let entry = entries::Occupied::key(&self.core, key.index(), key.generation())?;
        Some(entry.take().into_mut())
    }

    pub fn remove(&mut self, key: key::Handle<Tag, MAX>) -> Option<T> {
        let entry = entries::Occupied::key(&self.core, key.index(), key.generation())?;
        Some(entry.take().remove())
    }

    pub fn clear(&mut self) {
        self.core.clear();
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn try_entry(
        &'a mut self,
        key: key::Handle<Tag, MAX>,
    ) -> Lookup<'a, T, Tag, MAX, RECYCLE, PARTITIONS> {
        let entry = entries::Occupied::try_key(&self.core, key)?;
        Ok(slab::Occupied::new(entry))
    }
}

const _: () = {
    assert!(size_of::<slab::Occupied<entries::Busy<'static, u64>>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<slab::Occupied<entries::Busy<'static, u64>, (), false>>()
            == size_of::<slab::Occupied<entries::Busy<'static, u64>>>()
    );
    assert!(
        size_of::<
            slab::Occupied<entries::Busy<'static, u64, { u32::MAX }, entries::Exclusive, 1, u8>>,
        >() == size_of::<slab::Occupied<entries::Busy<'static, u64>>>()
    );
    assert!(
        size_of::<
            Option<
                slab::Occupied<
                    entries::Busy<'static, u64, { u32::MAX }, entries::Exclusive, 1, u8>,
                >,
            >,
        >() == size_of::<slab::Occupied<entries::Busy<'static, u64>>>()
    );
};

impl<'a, 'h, T, Tag, const MAX: u32>
    TryFrom<
        crate::depot::raw::Proof<
            (
                &'a mut slab::Store<T, Tag, MAX>,
                capacity::Key<'h, key::Handle<Tag, MAX>>,
            ),
            Physical,
        >,
    > for slab::Occupied<entries::Busy<'a, T, MAX>, Tag>
{
    type Error = key::Stale<'h, Tag, MAX>;

    fn try_from(
        proof: crate::depot::raw::Proof<
            (
                &'a mut slab::Store<T, Tag, MAX>,
                capacity::Key<'h, key::Handle<Tag, MAX>>,
            ),
            Physical,
        >,
    ) -> Result<Self, Self::Error> {
        let entry = entries::Occupied::try_bound(unsafe {
            proof.map(|(source, key)| (&source.core, key))
        })?;
        Ok(Self::new(entry))
    }
}

/// A branded exclusive slab owner with an explicit generation policy.
pub struct Store<
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
> where
    cycle::Mode<RECYCLE>: Policy,
{
    pub(in crate::depot::arena::slab) core:
        entries::Core<T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State<B, R> {
    Free,
    Reserved,
    Occupied,
    Busy(B),
    Retired(R),
}

/// The exact non-live-borrow refusal of one admitted slot.
#[derive(Debug)]
pub(in crate::depot::arena::slab) enum Refusal<R> {
    Free,
    Reserved,
    Retired(R),
    Stale,
}

/// The classification of one admitted slot against a retained generation.
#[derive(Debug)]
pub(in crate::depot::arena::slab) enum Admitted<B, R> {
    Exact,
    Refused(Refusal<R>),
    Borrowed(B),
}

impl<B, R> State<B, R> {
    /// Classifies one admitted slot, retaining the live-borrow witness for its caller.
    pub(in crate::depot::arena::slab) fn classify<const MAX: u32>(
        self,
        actual: key::Generation<MAX>,
        expected: key::Generation<MAX>,
    ) -> Admitted<B, R> {
        match self {
            Self::Free => Admitted::Refused(Refusal::Free),
            Self::Reserved => Admitted::Refused(Refusal::Reserved),
            Self::Retired(retired) => Admitted::Refused(Refusal::Retired(retired)),
            Self::Busy(borrow) => Admitted::Borrowed(borrow),
            Self::Occupied if actual != expected => Admitted::Refused(Refusal::Stale),
            Self::Occupied => Admitted::Exact,
        }
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Consumes an initialized-slot proof without repeating its lookup.
    pub fn value<P>(
        live: crate::depot::raw::Proof<
            (
                &'a slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>,
                links::Index,
            ),
            Initialized<P>,
        >,
    ) -> crate::depot::raw::Proof<(&'a T, links::Index), Initialized<P>> {
        // SAFETY: the native core retains the complete shared slab loan.
        entries::Core::read(unsafe { live.map(|(slab, index)| (&slab.core, index)) })
    }
}

/// One distinct physical slot retained by exclusive iteration.
pub enum Entry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    Vacant(slab::Vacant<entries::Pending<'a, T, MAX>, Tag, false>),
    Occupied(slab::Occupied<entries::Busy<'a, T, MAX>, Tag, false>),
}

/// An occupied slot whose next generation has been proved available.
pub struct Reuse<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, const ROOT: bool = true> {
    entry: slab::Occupied<entries::Busy<'a, T, MAX>, Tag, ROOT>,
    next: key::Generation<MAX>,
}

impl<'a, T, Tag, const MAX: u32, const ROOT: bool>
    TryFrom<slab::Occupied<entries::Busy<'a, T, MAX>, Tag, ROOT>> for Reuse<'a, T, Tag, MAX, ROOT>
{
    type Error = slab::Occupied<entries::Busy<'a, T, MAX>, Tag, ROOT>;

    fn try_from(entry: Self::Error) -> Result<Self, Self::Error> {
        let Some(next) = entry.entry.generation().checked_add(1) else {
            return Err(entry);
        };
        Ok(Self { entry, next })
    }
}

impl<'a, T, Tag, const MAX: u32, const ROOT: bool> Reuse<'a, T, Tag, MAX, ROOT> {
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        self.entry.key()
    }

    /// Replaces the value and advances its identity without rechecking capacity or generation.
    pub fn replace(self, value: T) -> (slab::Occupied<entries::Busy<'a, T, MAX>, Tag, ROOT>, T) {
        let (busy, prior) = self.entry.entry.replace(self.next, value);
        (
            slab::Occupied {
                entry: busy,
                tag: self.entry.tag,
            },
            prior,
        )
    }
}

/// The retained (owner, index, projector) selects a valid field of that initialized slot.
/// The field remains disjoint from mutable payloads for the owner's full returned loan.
pub enum Field {}

impl crate::depot::raw::Sealed for Field {}
impl crate::depot::Predicate for Field {}

type Lookup<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize> = Result<
    slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>,
    key::Error<Tag, MAX, convert::Infallible, <cycle::Mode<RECYCLE> as Policy>::Retired>,
>;
