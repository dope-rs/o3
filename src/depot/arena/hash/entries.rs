use std::{marker, mem, num};

use runtime::entry;

use crate::{
    base::num::bounded,
    depot::{
        arena::{
            hash::{self, runtime},
            slab::key,
        },
        map,
        queue::links,
    },
};

/// An initialized resident entry carrying its directory membership phase.
pub struct Live<'a, T, Tag = (), P = ()> {
    pub(super) core: &'a mut hash::Core<T, Tag>,
    pub(super) index: bounded::U32<0, { u32::MAX - 1 }>,
    phase: marker::PhantomData<fn(P) -> P>,
}

impl<'a, T, Tag> Live<'a, T, Tag, entry::Staged> {
    pub(super) fn index(
        entry: Self,
        bucket: map::raw::Vacant,
        hash: u64,
    ) -> entry::Live<'a, T, Tag, entry::Indexed> {
        let stored = unsafe { Bucket::new(bucket.index()) };
        unsafe { entry.core.directory.insert(bucket, hash, entry.index) };
        unsafe {
            entry.core.meta_mut(entry.index).bucket = stored;
        }
        entry.with_phase()
    }
}

impl<'a, T, Tag> Live<'a, T, Tag> {
    /// # Safety
    /// `index` must identify a live entry in `core`.
    pub(super) unsafe fn new(
        core: &'a mut hash::Core<T, Tag>,
        index: bounded::U32<0, { u32::MAX - 1 }>,
    ) -> Self {
        Self {
            core,
            index,
            phase: marker::PhantomData,
        }
    }

    pub(super) fn classify(self) -> entry::Entry<'a, T, Tag> {
        if unsafe { self.core.meta(self.index) }.bucket == Bucket::NONE {
            entry::Entry::Staged(self.with_phase())
        } else {
            entry::Entry::Indexed(self.with_phase())
        }
    }

    pub(super) fn indexed(self) -> Option<entry::Live<'a, T, Tag, entry::Indexed>> {
        if unsafe { self.core.meta(self.index) }.bucket == Bucket::NONE {
            None
        } else {
            Some(self.with_phase())
        }
    }
}

impl<T, Tag> Live<'_, T, Tag, entry::Staged> {
    pub(super) fn vacate(owner: &mut Self) {
        let position = unsafe { owner.core.meta(owner.index) }.slot;
        // SAFETY: this exclusively borrowed live entry retains a nonzero core count.
        let len = unsafe { num::NonZero::<u32>::new_unchecked(owner.core.len) };
        let last = len.get() - 1;
        owner.core.len = last;
        if position != last {
            let moved = unsafe { *owner.core.dense.as_ptr().add(last as usize) };
            unsafe {
                owner
                    .core
                    .dense
                    .as_ptr()
                    .add(position as usize)
                    .write(moved);
                owner.core.meta_mut(moved).slot = position;
            }
        }
        let free = owner.core.free;
        let meta = unsafe { owner.core.meta_mut(owner.index) };
        if let Some(next) = meta.generation.checked_add(1) {
            meta.generation = next;
            meta.slot = VACANT | free.map_or(Meta::FREE, links::Index::raw);
            owner.core.free = Some(owner.index.into());
        } else {
            meta.slot = VACANT | Meta::FREE;
        }
    }
}

const VACANT: u32 = 1 << 31;

#[derive(Clone, Copy, PartialEq)]
#[repr(transparent)]
pub(in crate::depot) struct Bucket(u32);

impl Bucket {
    pub(in crate::depot) const NONE: Self = Self(u32::MAX);

    /// # Safety
    /// `index` must belong to a resident-hash directory whose validated shape
    /// bounds every bucket index below `u32::MAX`.
    pub(in crate::depot) const unsafe fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub(in crate::depot) fn occupied<T, Tag>(
        indexed: &entry::Live<'_, T, Tag, entry::Indexed>,
    ) -> map::raw::Occupied {
        let bucket = unsafe { indexed.core.meta(indexed.index) }.bucket;
        unsafe { map::raw::Occupied::new(bucket.0 as usize) }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(in crate::depot) struct Meta {
    pub(in crate::depot) generation: key::Generation,
    pub(in crate::depot) slot: u32,
    pub(in crate::depot) bucket: Bucket,
}

impl Meta {
    const FREE: u32 = (1 << 31) - 1;

    #[inline]
    pub(in crate::depot) fn vacant(next: Option<links::Index>) -> Self {
        Self {
            generation: key::Generation::MIN,
            slot: VACANT | next.map_or(Self::FREE, links::Index::raw),
            bucket: Bucket::NONE,
        }
    }

    pub(in crate::depot) const fn is_live(&self) -> bool {
        self.slot & VACANT == 0
    }

    pub(in crate::depot) fn next(&self) -> Option<links::Index> {
        let next = self.slot & Self::FREE;
        if next == Self::FREE {
            None
        } else {
            // SAFETY: the packed free link retains its original allocation index.
            Some(unsafe { links::Index::new_raw(next) })
        }
    }
}

const _: () = assert!(mem::size_of::<Meta>() == 3 * mem::size_of::<u32>());
const _: () = assert!(mem::size_of::<Bucket>() == mem::size_of::<u32>());

impl<'a, T, Tag, P> Live<'a, T, Tag, P> {
    pub(super) fn with_phase<N>(self) -> entry::Live<'a, T, Tag, N> {
        entry::Live {
            core: self.core,
            index: self.index,
            phase: marker::PhantomData,
        }
    }
    pub(super) fn slot(&mut self) -> &mut mem::MaybeUninit<T> {
        unsafe { &mut *self.core.values.as_ptr().add(self.index.into_usize()) }
    }

    pub fn into_mut(self) -> &'a mut T {
        unsafe { self.core.value_mut(self.index) }
    }
}
