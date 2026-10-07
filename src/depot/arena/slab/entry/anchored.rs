use std::{marker, pin};

use cycle::Policy;

use crate::depot::arena::slab::{entries, key::cycle, storage};

/// One initialized slot of a pinned store.
///
/// The value keeps its address for the whole store loan, so only pinned
/// projections leave this guard; the slot bookkeeping stays in o3.
pub struct Anchored<
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
    pub(in crate::depot) entry:
        entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, (), S, I>,
    pub(in crate::depot) tag: marker::PhantomData<fn() -> Tag>,
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S, I>
    Anchored<'a, T, Tag, MAX, RECYCLE, PARTITIONS, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub fn get(&self) -> &T {
        self.entry.value()
    }

    pub fn value(&mut self) -> pin::Pin<&mut T> {
        // SAFETY: the occupied slot keeps this value at its storage address.
        unsafe { pin::Pin::new_unchecked(self.entry.value_mut()) }
    }

    /// Projects the value for the remainder of the owner's pinned loan.
    pub fn into_value(self) -> pin::Pin<&'a mut T> {
        // SAFETY: the occupied slot keeps this value at its storage address.
        unsafe { pin::Pin::new_unchecked(self.entry.into_mut()) }
    }

    /// Drops the value in place and returns its slot to the free list.
    pub fn discard(self) {
        self.entry.discard();
    }
}
