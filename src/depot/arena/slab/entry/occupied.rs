use std::marker;

use cycle::Policy;

use crate::{
    depot::{
        arena::slab::{
            self, Rest, entries,
            key::{self, cycle},
        },
        index::Initialized,
        queue::links,
    },
    mem::lease,
};

/// An initialized slot held through its exclusive native owner.
pub struct Occupied<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Consumes a live root projection into its selected exclusive slot.
    pub fn from_live<C, P>(
        live: Live<'a, T, Tag, C, P, MAX, RECYCLE, PARTITIONS>,
    ) -> (
        slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>, Tag>,
        C,
    ) {
        // SAFETY: the projection retains the original exclusive owner and context.
        let live = unsafe { live.map(|((slab, context), index)| ((&slab.core, context), index)) };
        let (busy, context) = live.into();
        (
            slab::Occupied {
                entry: busy,
                tag: marker::PhantomData,
            },
            context,
        )
    }

    pub(in crate::depot::arena::slab) fn new(
        entry: entries::Occupied<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
    ) -> Self {
        Self {
            entry: entry.take(),
            tag: marker::PhantomData,
        }
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, P, const ROOT: bool>
    slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>, Tag, ROOT>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        key::Handle::new(self.entry.index(), self.entry.generation())
    }

    pub fn get(&self) -> &T {
        self.entry.value()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.entry.value_mut()
    }

    pub fn into_mut(self) -> &'a mut T {
        self.entry.into_mut()
    }

    /// Withdraws the value while retaining its owner, identity, and operation plane.
    pub fn take(self) -> Withdrawn<'a, T, Tag, P, MAX, RECYCLE, PARTITIONS, ROOT> {
        let (pending, value) = self.entry.withdraw();
        (slab::Withdrawn::new(pending), value)
    }

    pub fn remove(self) -> T {
        self.entry.remove()
    }

    pub fn discard(self) {
        self.entry.discard();
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, P>
    slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>, Tag>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Splits this slot from read access to every other slot in its original owner.
    pub fn split(self) -> Split<'a, T, Tag, P, MAX, RECYCLE, PARTITIONS> {
        let rest = Rest::new(&self.entry);
        (
            slab::Occupied {
                entry: self.entry,
                tag: self.tag,
            },
            rest,
        )
    }

    /// Borrows a live neighbor through the original storage owner's exclusive loan.
    pub fn read<Q>(live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<Q>>) -> &T {
        // SAFETY: the occupied guard retains the complete native owner and slot proof.
        entries::Busy::read(unsafe { live.map(|(entry, index)| (&entry.entry, index)) })
    }

    /// Projects a live neighbor while retaining the whole storage owner's exclusive loan.
    pub fn project<Q>(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<Q>>,
    ) -> (key::Handle<Tag, MAX>, &T) {
        let index = *live.key();
        // SAFETY: the occupied guard owns this exact core for its full borrow.
        let live = unsafe { live.map(|(entry, index)| (&entry.entry, index)) };
        let (epoch, value) = entries::Busy::project(live);
        (key::Handle::new(index.raw(), epoch), value)
    }
}

type Live<'a, T, Tag, C, P, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize> =
    crate::depot::raw::Proof<
        (
            (&'a mut slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>, C),
            links::Index,
        ),
        Initialized<P>,
    >;

type Withdrawn<
    'a,
    T,
    Tag,
    P,
    const MAX: u32,
    const RECYCLE: bool,
    const PARTITIONS: usize,
    const ROOT: bool,
> = (
    slab::Withdrawn<
        crate::mem::Lease<
            entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>,
            lease::Open,
        >,
        Tag,
        ROOT,
    >,
    T,
);

type Split<'a, T, Tag, P, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize> = (
    slab::Occupied<
        entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P>,
        Tag,
        false,
    >,
    Rest<'a, T, Tag, MAX, RECYCLE, PARTITIONS, P>,
);
