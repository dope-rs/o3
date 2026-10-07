//! Generation-keyed owned payloads with intrusive FIFO scheduling.

use std::{marker, mem, ops};

pub use batch::Batch;
use batch::reserved::Reserved;
pub use cursor::Cursor;
pub use error::Error;
pub use full::Full;
pub use scope::{Scope, edit::Edit, key::Key};
pub use slot::{Slot, vacant::Vacant};

use crate::depot::{
    arena::slab::key,
    array::Index,
    capacity,
    queue::{indexed::slab, links, tagged},
    raw::Live,
};

mod error;
mod full;

mod batch;
mod cursor;
pub mod phase;
mod scope;
mod slot;

pub struct Slab<
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    H: crate::depot::raw::IndexedHook = links::Hook,
    const N: usize = 0,
    C: crate::depot::raw::Count = capacity::Count,
> {
    pub(in crate::depot) block: crate::depot::raw::IndexedBlock<T, Tag, MAX, H, N, C>,
    chain: H::Chain,
}

impl<
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> Slab<T, Tag, MAX, H, N, C>
{
    pub fn try_from(capacity: C) -> Result<Self, crate::alloc::Error> {
        const { assert!(N <= u8::MAX as usize) };
        let shape = tagged::Domain::<[H::Head; N], C>::new(capacity);
        Ok(Self {
            block: crate::depot::raw::IndexedBlock::try_new(shape)?,
            chain: H::Chain::default(),
        })
    }

    /// Returns physical capacity, including slots whose generations have retired.
    pub fn capacity(&self) -> C {
        self.block.shape.capacity()
    }

    pub fn len(&self) -> usize {
        self.block.state().len as usize
    }

    /// Returns slots available for admission, excluding occupied and retired slots.
    pub fn available(&self) -> usize {
        self.block.state().available as usize
    }

    pub fn vacant(&mut self) -> Option<slab::Vacant<'_, T, Tag, MAX, H, N, C, phase::Vacant>> {
        let (core, [index]) = Reserved::new(self)?.into_parts();
        Some(slab::Vacant {
            entry: slab::Slot {
                core,
                index,
                phase: phase::Vacant,
            },
        })
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_full(&self) -> bool {
        self.available() == 0
    }

    pub fn try_get(&self, handle: slab::Handle<Tag, MAX>) -> Result<&T, slab::Error<Tag, MAX, C>> {
        Ok(handle.resolve(self)?.into())
    }

    pub fn try_entry(
        &mut self,
        key: slab::Handle<Tag, MAX>,
    ) -> Result<slab::Slot<'_, T, Tag, MAX, H, N, C>, slab::Error<Tag, MAX, C>> {
        Ok(key.resolve(self)?.into())
    }
}

impl<T, Tag, const MAX: u32, C: crate::depot::raw::Count> Slab<T, Tag, MAX, links::Hook, 0, C> {
    pub fn entry(
        &mut self,
        key: slab::Handle<Tag, MAX>,
    ) -> Option<slab::Slot<'_, T, Tag, MAX, links::Hook, 0, C>> {
        Some(key.resolve(self).ok()?.into())
    }

    pub fn front_pair(&self) -> Option<(slab::Handle<Tag, MAX>, &T)> {
        let index = self.chain.first()?;
        Some(unsafe { self.block.pair(index) })
    }
}

impl<T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Slab<T, Tag, MAX, tagged::Hook, N, C>
{
    pub fn select<I, const M: usize>(
        &mut self,
        order: [(Index<N>, I); M],
    ) -> Option<(I, slab::Slot<'_, T, Tag, MAX, tagged::Hook, N, C>)> {
        let (item, entry) = tagged::Endpoint::select(self, order)?;
        let (core, index) = entry.into_inner();
        Some((
            item,
            slab::Slot {
                core,
                index,
                phase: phase::Occupied,
            },
        ))
    }
    pub fn front_pair(&self, lane: Index<N>) -> Option<(slab::Handle<Tag, MAX>, &T)> {
        let (lane, index) = tagged::Endpoint::indexed(self, lane).front()?.into_inner();
        Some(unsafe { lane.into_store().block.pair(index) })
    }
}

impl<T, Tag, const MAX: u32, C: crate::depot::raw::Count> Slab<T, Tag, MAX, links::Hook, 0, C> {
    pub fn pop_front(&mut self) -> Option<T> {
        let index = self.chain.first()?;
        // SAFETY: this exclusive owner retains the selected reciprocal front
        // in its original chain and initialized hook allocation through commit.
        let (_, detached) =
            unsafe { crate::depot::raw::Hooks::chain_pop(&self.block, &self.chain, index) };
        detached.idle();
        Some(
            slab::Slot {
                core: self,
                index,
                phase: phase::Detached,
            }
            .read(),
        )
    }
}

/// A compact physical index and generation in one typed slab domain.
#[derive(Debug)]
#[repr(transparent)]
pub struct Handle<Tag = (), const MAX: u32 = { u32::MAX }> {
    parts: key::Parts<MAX>,
    marker: marker::PhantomData<*mut Tag>,
}

impl<Tag, const MAX: u32> Handle<Tag, MAX> {
    pub(in crate::depot) fn resolve<
        T,
        O,
        H: crate::depot::raw::IndexedHook,
        const N: usize,
        C: crate::depot::raw::Count,
    >(
        self,
        owner: O,
    ) -> Result<Live<O, links::Index, H>, slab::Error<Tag, MAX, C>>
    where
        O: ops::Deref<Target = Slab<T, Tag, MAX, H, N, C>>,
    {
        let key = self;
        let core = &*owner;
        let index = key.index();
        let ready = core.block.state().initialized;
        if index >= ready {
            let capacity = core.capacity();
            return Err(if index as usize >= capacity.count().get() {
                Error::Index { key, capacity }
            } else {
                Error::Uninitialized {
                    key,
                    initialized: ready,
                }
            });
        }
        // SAFETY: index lies in this retained block's initialized metadata prefix.
        let meta = unsafe { core.block.metadata.add(index as usize).as_ref() };
        let actual = meta.generation;
        if !H::is_linked(&meta.hook, links::Index::from(key)) {
            // SAFETY: this initialized unlinked hook retains its free/retired successor.
            return Err(if unsafe { meta.hook.next() } == index {
                Error::Retired { key, actual }
            } else {
                Error::Free { key, actual }
            });
        }
        if actual != key.generation() {
            return Err(Error::Generation { key, actual });
        }
        // SAFETY: the retained owner fixes this initialized, live generation and hook domain.
        Ok(unsafe { crate::depot::raw::Proof::new((owner, links::Index::from(key))) })
    }

    pub(in crate::depot) const fn new(index: links::Index, epoch: key::Generation<MAX>) -> Self {
        Self {
            parts: key::Parts::new(index.raw(), epoch),
            marker: marker::PhantomData,
        }
    }

    pub const fn index(self) -> u32 {
        self.parts.index()
    }

    pub const fn generation(self) -> key::Generation<MAX> {
        self.parts.generation()
    }
}

impl<Tag, const MAX: u32> From<Handle<Tag, MAX>> for links::Index {
    fn from(handle: Handle<Tag, MAX>) -> Self {
        // SAFETY: the sole Handle constructor retains a compact index excluding END.
        unsafe { Self::new_raw(handle.parts.index()) }
    }
}

impl<Tag, const MAX: u32> Clone for Handle<Tag, MAX> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, const MAX: u32> Copy for Handle<Tag, MAX> {}

impl<Tag, const MAX: u32> PartialEq for Handle<Tag, MAX> {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl<Tag, const MAX: u32> Eq for Handle<Tag, MAX> {}

const _: () = {
    assert!(mem::size_of::<crate::depot::raw::IndexedState>() == 4 * mem::size_of::<u32>());
    assert!(
        mem::size_of::<crate::depot::raw::IndexedMeta<{ u32::MAX }, links::Hook>>()
            == 3 * mem::size_of::<u32>()
    );
    assert!(
        mem::align_of::<crate::depot::raw::IndexedMeta<{ u32::MAX }, links::Hook>>()
            == mem::align_of::<links::Chain<()>>()
    );
    assert!(mem::size_of::<Slab<u64, (), { u32::MAX }>>() == 4 * mem::size_of::<usize>());
};
