use std::{marker, pin};

use cycle::Policy;

use crate::depot::arena::slab::{
    entries,
    key::{self, cycle},
    storage,
};

/// One reserved slot of a pinned store awaiting its first value.
///
/// The pinned owner forwards its own lifetime and tag; the free list and the
/// generation check stay in o3 so both storage policies share them.
pub struct Anchor<
    'a,
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
    pub(in crate::depot::arena::slab) entry:
        entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, (), S, I>,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S, I>
    Anchor<'a, T, Tag, MAX, RECYCLE, PARTITIONS, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub fn index(&self) -> u32 {
        self.entry.index().raw()
    }

    fn key(&self) -> key::Handle<Tag, MAX> {
        key::Handle::new(self.index(), self.entry.generation())
    }

    /// Installs the first value and returns its identity.
    pub fn insert(self, value: T) -> key::Handle<Tag, MAX> {
        let key = self.key();
        drop(self.entry.install(value));
        key
    }

    /// Installs the first value and projects it at its final address.
    pub fn occupy(self, value: T) -> (key::Handle<Tag, MAX>, pin::Pin<&'a mut T>) {
        let key = self.key();
        let busy = self.entry.install(value);
        // SAFETY: the installed value stays at this slot address for the pinned
        // owner's full returned loan; the exclusive slot exposes no move-out API.
        (key, unsafe { pin::Pin::new_unchecked(busy.into_mut()) })
    }
}
