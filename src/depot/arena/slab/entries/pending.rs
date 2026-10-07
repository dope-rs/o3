use std::{convert, marker, mem, num::NonZero};

use cycle::Policy;

use crate::{
    depot::{
        arena::slab::{
            self, entries, external,
            key::{self, cycle},
            storage::{self, Plane},
        },
        queue::links,
    },
    mem::{Lease, lease},
};

pub struct Pending<
    'a,
    T,
    const MAX: u32 = { u32::MAX },
    M: entries::Mode = entries::Exclusive<false>,
    const PARTITIONS: usize = 1,
    P = (),
    S = storage::Heap,
    I = storage::Eager,
> where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot::arena::slab::entries) plane: marker::PhantomData<fn(P) -> P>,
    pub(in crate::depot::arena::slab) core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
    pub(in crate::depot::arena::slab::entries) slot: &'a crate::depot::raw::SlabSlot<
        T,
        key::Generation<MAX>,
        M::Busy,
        <M::Cycle as Policy>::Retired,
    >,
}

impl<'a, T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, P, S, I>
    Pending<'a, T, MAX, M, PARTITIONS, P, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot::arena::slab) fn index(&self) -> links::Index {
        // SAFETY: the retained slot belongs to this allocation, whose capacity excludes END.
        unsafe { self.slot.index(self.core.slots.as_ref()) }
    }

    pub(in crate::depot::arena::slab) fn generation(&self) -> key::Generation<MAX> {
        self.slot.generation.get()
    }

    /// Initializes the reservation and publishes its dense position.
    /// The reserved slot proves `pos < capacity <= u32::MAX`.
    pub(in crate::depot::arena::slab) fn install(
        self,
        value: T,
    ) -> entries::Busy<'a, T, MAX, M, PARTITIONS, P, S, I> {
        let hold = mem::ManuallyDrop::new(self);
        let index = hold.index();
        let pos = hold.core.len.get();
        // SAFETY: this reservation exclusively owns the inactive value storage
        // and proves that the next dense position is below capacity. Its index
        // is initialized before publishing the longer live prefix.
        unsafe {
            hold.slot.write_value(value);
            hold.core
                .slots
                .tail()
                .get_unchecked(pos as usize)
                .set(mem::MaybeUninit::new(index.raw()));
        }
        hold.core.len.set(pos + 1);
        hold.slot.position.set(pos);
        hold.slot.state.set(M::TAKEN);
        entries::Busy {
            plane: hold.plane,
            core: hold.core,
            slot: hold.slot,
        }
    }

    pub(in crate::depot::arena::slab) fn reject(self) {
        match <M::Cycle as Policy>::next(self.generation()) {
            Ok(epoch) => self.slot.generation.set(epoch),
            Err(retired) => {
                let hold = mem::ManuallyDrop::new(self);
                hold.slot.state.set(slab::State::Retired(retired));
                if let Some(count) = hold.core.slots.retired() {
                    count.set(count.get().wrapping_add(1));
                }
            }
        }
    }
}

impl<T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, P, S, I> Drop
    for Pending<'_, T, MAX, M, PARTITIONS, P, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    fn drop(&mut self) {
        let index = entries::SlotIndex(self.index().raw());
        let head = self.core.partition(index).replace(index.get());
        // SAFETY: this reservation owns the non-value union member of its exact slot.
        unsafe {
            self.slot.set_links(crate::depot::raw::SlabLinks {
                next: head,
                prev: entries::NONE,
            });
        }
        self.slot.state.set(slab::State::Free);
        if head != entries::NONE {
            // SAFETY: this owner's previous free head remains free and in bounds.
            unsafe {
                self.core
                    .slots
                    .as_ref()
                    .get_unchecked(head as usize)
                    .set_prev(index.get());
            }
        }
    }
}

impl<T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, P, S, I> crate::mem::Return
    for Pending<'_, T, MAX, M, PARTITIONS, P, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    type Item = ();

    fn return_item(self, (): ()) {
        self.reject();
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool>
    slab::Vacant<entries::Pending<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Issues this identity; dropping before installation retires its generation.
    pub fn reserve(self) -> slab::Reserved<'a, T, Tag, MAX, RECYCLE> {
        slab::Vacant {
            entry: Lease::open(self.entry, ()),
            tag: self.tag,
        }
    }

    pub fn insert(self, value: T) -> key::Handle<Tag, MAX> {
        let key = key::Handle::new(self.entry.index().raw(), self.entry.generation());
        drop(self.entry.install(value));
        key
    }

    pub fn occupy(
        self,
        value: T,
    ) -> slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag> {
        slab::Borrowed {
            entry: self.entry.install(value),
            tag: marker::PhantomData,
        }
    }

    pub fn reject(self) {
        self.entry.reject();
    }

    pub fn stage(
        self,
        value: T,
    ) -> slab::Staged<
        crate::mem::Lease<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, lease::Open>,
        Tag,
    > {
        slab::Staged {
            entry: Lease::open(self.entry.install(value), ()),
            tag: marker::PhantomData,
        }
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool> slab::Reserved<'a, T, Tag, MAX, RECYCLE>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn position(&self) -> NonZero<u32> {
        self.entry.sink().index().position()
    }

    pub fn key(&self) -> key::Handle<Tag, MAX> {
        let entry = self.entry.sink();
        key::Handle::new(entry.index().raw(), entry.generation())
    }

    pub fn occupy(
        self,
        value: T,
    ) -> slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag> {
        slab::Borrowed {
            entry: self.entry.into_parts().0.install(value),
            tag: self.tag,
        }
    }
}

impl<'a, T, G: external::Generation, Tag, const PHYSICAL_MAX: u32>
    external::Vacant<
        entries::Pending<'a, external::Stamped<T, G>, PHYSICAL_MAX, entries::Interior<true>, 1>,
        Tag,
        true,
        G,
    >
{
    pub fn key(&self) -> external::Key<G, Tag> {
        external::Key::new(self.entry.index().raw(), self.state)
    }

    pub fn insert(self, value: T) -> external::Key<G, Tag> {
        let key = self.key();
        self.stage(value).publish();
        key
    }

    pub fn stage(self, value: T) -> Staging<'a, T, G, Tag, PHYSICAL_MAX> {
        let epoch = self.state;
        self.into_native()
            .stage(external::Stamped {
                generation: epoch,
                value,
            })
            .with_state(epoch)
    }
}

/// An issued slot awaiting restoration; dropping it retires the original identity.
impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, P, const ROOT: bool>
    slab::Withdrawn<
        crate::mem::Lease<
            entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>,
            lease::Open,
        >,
        Tag,
        ROOT,
    >
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub(in crate::depot::arena::slab) fn new(
        pending: entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>,
    ) -> Self {
        Self {
            entry: Lease::open(pending, ()),
            tag: marker::PhantomData,
        }
    }

    pub fn insert(
        self,
        value: T,
    ) -> slab::Occupied<
        entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>,
        Tag,
        ROOT,
    > {
        let (pending, ()) = self.entry.into_parts();
        slab::Occupied {
            entry: pending.install(value),
            tag: marker::PhantomData,
        }
    }
}

const _: () = {
    assert!(
        size_of::<
            slab::Withdrawn<
                crate::mem::Lease<entries::Pending<'static, u64>, crate::mem::lease::Open>,
            >,
        >() == 2 * size_of::<usize>()
    );
    assert!(
        size_of::<
            Option<
                slab::Withdrawn<
                    crate::mem::Lease<entries::Pending<'static, u64>, crate::mem::lease::Open>,
                >,
            >,
        >() == size_of::<
            slab::Withdrawn<
                crate::mem::Lease<entries::Pending<'static, u64>, crate::mem::lease::Open>,
            >,
        >()
    );
};

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, const ROOT: bool>
    slab::Vacant<entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag, ROOT>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub(in crate::depot::arena::slab) fn from_pending(
        pending: entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
    ) -> Self {
        Self {
            entry: pending,
            tag: marker::PhantomData,
        }
    }

    pub fn insert(self, value: T) -> key::Handle<Tag, MAX> {
        let key = key::Handle::new(self.entry.index().raw(), self.entry.generation());
        self.commit(value);
        key
    }

    pub(in crate::depot::arena::slab) fn commit(self, value: T) {
        drop(self.entry.install(value));
    }

    pub fn insert_with(
        self,
        build: impl FnOnce(key::Handle<Tag, MAX>) -> T,
    ) -> key::Handle<Tag, MAX> {
        let key = key::Handle::new(self.entry.index().raw(), self.entry.generation());
        let value = build(key);
        self.commit(value);
        key
    }

    pub fn stage_with(
        self,
        build: impl FnOnce(links::Index) -> T,
    ) -> slab::Staged<
        crate::mem::Lease<
            entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
            lease::Open,
        >,
        Tag,
        ROOT,
    > {
        let value = build(self.entry.index());
        self.stage(value)
    }

    pub fn stage(
        self,
        value: T,
    ) -> slab::Staged<
        crate::mem::Lease<
            entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
            lease::Open,
        >,
        Tag,
        ROOT,
    > {
        slab::Staged {
            entry: Lease::open(self.entry.install(value), ()),
            tag: self.tag,
        }
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::Vacant<entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Splits the whole owner loan into one reserved slot and a distinct occupied slot.
    pub fn try_split(
        self,
        key: key::Handle<Tag, MAX>,
    ) -> Replaced<'a, T, Tag, MAX, RECYCLE, PARTITIONS> {
        let entry = entries::Occupied::try_key(self.entry.core, key)?;
        Ok((
            slab::Vacant::from_pending(self.entry),
            slab::Occupied {
                entry: entry.take(),
                tag: marker::PhantomData,
            },
        ))
    }
}

type Staging<'a, T, G, Tag, const MAX: u32> = external::Staged<
    crate::mem::Lease<
        entries::Busy<'a, external::Stamped<T, G>, MAX, entries::Interior<true>, 1>,
        lease::Open,
    >,
    Tag,
    true,
    G,
>;

type Replaced<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize> = Result<
    (
        slab::Vacant<
            entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
            Tag,
            false,
        >,
        slab::Occupied<
            entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
            Tag,
            false,
        >,
    ),
    key::Error<Tag, MAX, convert::Infallible, <cycle::Mode<RECYCLE> as Policy>::Retired>,
>;
