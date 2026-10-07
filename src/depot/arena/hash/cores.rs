use std::{
    marker,
    mem::{MaybeUninit, needs_drop, size_of},
    ptr,
};

use runtime::entry;

use crate::{
    base::{marker::ThreadBound, num::bounded},
    depot::{
        arena::{
            hash::{self, runtime},
            slab::key,
        },
        map,
        queue::links,
    },
};

pub(in crate::depot) struct Core<T, Tag = ()> {
    pub(in crate::depot) directory: map::raw::Table<bounded::U32<0, { u32::MAX - 1 }>>,
    thread: marker::PhantomData<ThreadBound>,
    pub(in crate::depot) meta: ptr::NonNull<hash::Meta>,
    pub(in crate::depot) dense: ptr::NonNull<bounded::U32<0, { u32::MAX - 1 }>>,
    pub(in crate::depot) values: ptr::NonNull<MaybeUninit<T>>,
    shape: hash::Layout<T>,
    pub(in crate::depot) len: u32,
    pub(in crate::depot) free: Option<links::Index>,
    tag: marker::PhantomData<fn(Tag) -> Tag>,
}

impl<T, Tag> Core<T, Tag> {
    pub(in crate::depot) fn try_new(
        layout: hash::Parts<T>,
    ) -> Result<Self, crate::alloc::Exhausted> {
        let capacity = layout.shape.capacity();
        let fields = layout.bundle.try_allocate()?;
        // SAFETY: the freshly allocated bundle carries exactly the directory planes
        // admitted by `layout.buckets`, which one `Layout` proved for this recipe.
        let dir =
            unsafe { map::raw::Table::from_fields(fields.prior().prior().prior(), layout.buckets) };
        let meta = fields.prior().prior().tail();
        let dense = fields.prior().tail();
        let values = fields.tail();
        for index in 0..capacity {
            let next = if index + 1 == capacity {
                None
            } else {
                // SAFETY: `index + 1 < capacity <= Index::END` names one slot in this core's domain.
                Some(unsafe { links::Index::new_usize(index + 1) })
            };
            // SAFETY: `index < capacity` addresses one metadata slot of this allocation,
            // which is uninitialized until this write and never read before it.
            unsafe { meta.as_ptr().add(index).write(hash::Meta::vacant(next)) };
        }
        Ok(Self {
            directory: dir,
            thread: marker::PhantomData,
            meta,
            dense,
            values,
            shape: layout.shape,
            len: 0,
            free: if capacity == 0 {
                None
            } else {
                // SAFETY: a nonempty core admits slot `0`, which is below `Index::END`.
                Some(unsafe { links::Index::new_usize(0) })
            },
            tag: marker::PhantomData,
        })
    }

    pub(in crate::depot) const fn capacity(&self) -> usize {
        self.shape.capacity()
    }

    /// # Safety
    /// `index` must be below this core's capacity, so it addresses one initialized
    /// metadata slot of this allocation.
    pub(in crate::depot) unsafe fn meta(
        &self,
        index: bounded::U32<0, { u32::MAX - 1 }>,
    ) -> &hash::Meta {
        // SAFETY: the caller bounds `index` by capacity; this core initialized every such slot in `try_new`.
        unsafe { &*self.meta.as_ptr().add(index.into_usize()) }
    }

    /// # Safety
    /// `index` must be below this core's capacity, so it addresses one initialized
    /// metadata slot of this allocation.
    pub(in crate::depot) unsafe fn meta_mut(
        &mut self,
        index: bounded::U32<0, { u32::MAX - 1 }>,
    ) -> &mut hash::Meta {
        // SAFETY: the caller bounds `index` by capacity, and the exclusive borrow keeps the slot unique.
        unsafe { &mut *self.meta.as_ptr().add(index.into_usize()) }
    }

    /// # Safety
    /// `index` must be below this core's capacity and name a live, initialized value.
    pub(in crate::depot) unsafe fn value(&self, index: bounded::U32<0, { u32::MAX - 1 }>) -> &T {
        // SAFETY: the caller bounds `index` by capacity and proves the slot initialized.
        unsafe { (&*self.values.as_ptr().add(index.into_usize())).assume_init_ref() }
    }

    /// # Safety
    /// `index` must be below this core's capacity and name a live, initialized value.
    pub(in crate::depot) unsafe fn value_mut(
        &mut self,
        index: bounded::U32<0, { u32::MAX - 1 }>,
    ) -> &mut T {
        // SAFETY: the caller bounds `index` by capacity, proves the slot initialized,
        // and the exclusive borrow keeps the value unique.
        unsafe { (&mut *self.values.as_ptr().add(index.into_usize())).assume_init_mut() }
    }

    pub(in crate::depot) fn entry(
        &mut self,
        key: key::Handle<runtime::Domain<Tag>>,
    ) -> Option<hash::Entry<'_, T, Tag>> {
        Some(hash::Position::try_live(self, key).ok()?.into_entry())
    }

    pub(in crate::depot) fn entry_at(&mut self, index: usize) -> Option<hash::Entry<'_, T, Tag>> {
        Some(hash::Position::new(self, index)?.live()?.into_entry())
    }

    /// # Safety
    /// `index` must be below this core's capacity, so it addresses one initialized
    /// metadata slot whose generation names this key's exact slot.
    pub(in crate::depot) unsafe fn key_for(
        &self,
        index: bounded::U32<0, { u32::MAX - 1 }>,
    ) -> key::Handle<runtime::Domain<Tag>> {
        // SAFETY: the caller bounds `index` by capacity; `meta` reads that initialized slot.
        key::Handle::new(index.get(), unsafe { self.meta(index) }.generation)
    }
}

// SAFETY: each directory read keeps the core's allocation, slot domain and live slots unchanged.
unsafe impl<'a, T, Tag> map::raw::Query for &'a mut Core<T, Tag> {
    const SPARE: bool = true;
    type Payload = bounded::U32<0, { u32::MAX - 1 }>;
    type Value = T;
    type Occupied = entry::Live<'a, T, Tag, entry::Indexed>;

    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.directory
    }

    fn value(entry: map::Found<&Self>) -> &T {
        let (owner, payload) = entry.project();
        let index = *payload;
        // SAFETY: the directory retains only live payloads within its domain, so this index is initialized.
        unsafe { Core::value(owner, index) }
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        // SAFETY: the occupied bucket retains one live entry at this bounded index.
        unsafe { hash::Entry::new(owner, index) }.with_phase()
    }
}

// SAFETY: the vacancy retains the same core and publishes its exact hash and payload.
unsafe impl<'a, T, Tag> map::raw::Insertion for &'a mut Core<T, Tag> {
    type Vacant = entry::Vacancy<'a, T, Tag>;

    fn vacant(entry: map::Vacancy<Self>, hash: u64) -> Option<Self::Vacant> {
        entry::Vacancy::new(entry, hash)
    }
}

// SAFETY: the exclusive core borrow keeps the directory and reverse indices uniquely owned.
unsafe impl<T, Tag> map::raw::Mutation for &mut Core<T, Tag> {
    /// # Safety
    /// The caller preserves this exact directory and every live payload it holds.
    unsafe fn table_mut(&mut self) -> &mut map::raw::Table<Self::Payload> {
        &mut self.directory
    }

    /// # Safety
    /// Both buckets belong to this core; `next` is live with `hash` and `hole` is vacant.
    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, hash: u64) {
        // SAFETY: both buckets belong to this directory; `next` is live with `hash` and `hole` is vacant.
        let (moved, index) = unsafe { self.directory.shift(hole, next, hash) };
        let index = *index;
        // SAFETY: a directory shift reports an index below capacity, so its metadata slot is initialized.
        unsafe { self.meta_mut(index).bucket = hash::Bucket::new(moved.index()) };
    }
}

impl<T, Tag> Drop for Core<T, Tag> {
    fn drop(&mut self) {
        let layout = hash::Parts::from(self.shape);
        let pointer = self.directory.base();
        if needs_drop::<T>() {
            for position in (0..self.len as usize).rev() {
                // SAFETY: the dense prefix names each live value once; terminal
                // ownership preserves the allocation through reverse-order destruction.
                unsafe {
                    let index = *self.dense.as_ptr().add(position);
                    (*self.values.as_ptr().add(index.into_usize())).assume_init_drop();
                }
            }
        }
        // SAFETY: this exclusive owner reconstructs the recipe-bound bundle and
        // releases the allocation once, after every initialized value is cleared.
        unsafe { layout.bundle.deallocate(pointer) };
    }
}

const _: () = assert!(size_of::<Core<u64>>() == 9 * size_of::<usize>());

impl<T, Tag> Core<T, Tag> {
    pub(in crate::depot) const fn len(&self) -> usize {
        self.len as usize
    }

    pub(in crate::depot) fn get(&self, key: key::Handle<runtime::Domain<Tag>>) -> Option<&T> {
        Some(hash::Position::try_live(self, key).ok()?.value())
    }

    pub(in crate::depot) fn key_at(
        &self,
        index: usize,
    ) -> Option<key::Handle<runtime::Domain<Tag>>> {
        Some(hash::Position::new(self, index)?.live()?.key())
    }

    pub(in crate::depot) fn key_value_at(
        &self,
        index: usize,
    ) -> Option<(key::Handle<runtime::Domain<Tag>>, &T)> {
        let present = hash::Position::new(self, index)?.live()?;
        Some((present.key(), present.value()))
    }

    pub(in crate::depot) fn values(&self) -> impl Iterator<Item = &T> {
        let dense = self.dense.as_ptr();
        let values = self.values.as_ptr();
        // SAFETY: `0..len` names the initialized dense prefix, so each recorded index
        // is initialized in the values plane and visited once.
        (0..self.len as usize).map(move |position| unsafe {
            let index = *dense.add(position);
            (&*values.add(index.into_usize())).assume_init_ref()
        })
    }
}

unsafe impl<T, Tag> map::raw::Query for &Core<T, Tag> {
    const SPARE: bool = true;
    type Payload = bounded::U32<0, { u32::MAX - 1 }>;
    type Value = T;
    type Occupied = key::Handle<runtime::Domain<Tag>>;

    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.directory
    }

    fn value(entry: map::Found<&Self>) -> &T {
        let (owner, payload) = entry.project();
        let index = *payload;
        unsafe { owner.value(index) }
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        unsafe { owner.key_for(index) }
    }
}
