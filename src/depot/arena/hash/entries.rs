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

pub(in crate::depot) struct Entry<'a, T, Tag = ()> {
    pub(in crate::depot) core: &'a mut hash::Core<T, Tag>,
    pub(in crate::depot) index: bounded::U32<0, { u32::MAX - 1 }>,
    len: num::NonZero<u32>,
}

impl<'a, T, Tag> Entry<'a, T, Tag> {
    pub(in crate::depot) fn index(
        staged: entry::Live<'a, T, Tag, entry::Staged>,
        bucket: map::raw::Vacant,
        hash: u64,
    ) -> entry::Live<'a, T, Tag, entry::Indexed> {
        let stored = unsafe { Bucket::new(bucket.index()) };
        let entry = staged.raw;
        unsafe { entry.core.directory.insert(bucket, hash, entry.index) };
        unsafe {
            entry.core.meta_mut(entry.index).bucket = stored;
        }
        entry.with_phase()
    }
}

impl<'a, T, Tag> Entry<'a, T, Tag> {
    /// # Safety
    /// `index` must identify a live entry in `core`.
    pub(in crate::depot) unsafe fn new(
        core: &'a mut hash::Core<T, Tag>,
        index: bounded::U32<0, { u32::MAX - 1 }>,
    ) -> Self {
        Self {
            // SAFETY: a live hash core retains a nonzero entry count.
            len: unsafe { num::NonZero::<u32>::new_unchecked(core.len) },
            core,
            index,
        }
    }

    pub(in crate::depot) fn classify(self) -> entry::Entry<'a, T, Tag> {
        if unsafe { self.core.meta(self.index) }.bucket == Bucket::NONE {
            entry::Entry::Staged(self.with_phase())
        } else {
            entry::Entry::Indexed(self.with_phase())
        }
    }

    pub(in crate::depot) fn indexed(self) -> Option<entry::Live<'a, T, Tag, entry::Indexed>> {
        if unsafe { self.core.meta(self.index) }.bucket == Bucket::NONE {
            None
        } else {
            Some(self.with_phase())
        }
    }
}

impl<T, Tag> Entry<'_, T, Tag> {
    pub(in crate::depot) fn vacate(staged: &mut entry::Live<'_, T, Tag, entry::Staged>) {
        let owner = &mut staged.raw;
        let position = unsafe { owner.core.meta(owner.index) }.slot;
        owner.core.len = owner.len.get() - 1;
        let last = owner.core.len;
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

const _: () = {
    assert!(mem::size_of::<Entry<'static, (), ()>>() == 2 * mem::size_of::<usize>());
};

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
        let owner = &indexed.raw;
        let bucket = unsafe { owner.core.meta(owner.index) }.bucket;
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

impl<'a, T, Tag> Entry<'a, T, Tag> {
    pub(in crate::depot) fn with_phase<P>(self) -> entry::Live<'a, T, Tag, P> {
        entry::Live {
            raw: self,
            phase: marker::PhantomData,
        }
    }
    pub(in crate::depot) fn slot(&mut self) -> &mut mem::MaybeUninit<T> {
        unsafe { &mut *self.core.values.as_ptr().add(self.index.into_usize()) }
    }

    pub(in crate::depot) fn into_mut(self) -> &'a mut T {
        unsafe { self.core.value_mut(self.index) }
    }
}
