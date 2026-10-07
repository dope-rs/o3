use std::{mem, ops, ptr};

use slab::{Vacant, phase};

use crate::depot::{array::Index, queue::links, raw::Live};
pub(super) mod vacant;

use crate::depot::{
    capacity,
    queue::{indexed::slab, tagged},
};

/// An initialized slot retaining its owner and exact membership phase.
#[must_use]
pub struct Slot<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    H: crate::depot::raw::IndexedHook = links::Hook,
    const N: usize = 0,
    C: crate::depot::raw::Count = capacity::Count,
    P: phase::State = phase::Occupied,
> {
    pub(in crate::depot) core: &'a mut slab::Slab<T, Tag, MAX, H, N, C>,
    pub(in crate::depot) index: links::Index,
    pub(in crate::depot) phase: P,
}

impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
    P: phase::State,
> Slot<'a, T, Tag, MAX, H, N, C, P>
{
    pub(in crate::depot) fn value(&self) -> *mut mem::MaybeUninit<T> {
        // SAFETY: this admitted slot belongs to an allocated block.
        unsafe { self.core.block.values().as_ptr().add(self.index.get()) }
    }
    pub fn key(&self) -> slab::Handle<Tag, MAX> {
        // SAFETY: this slot retains its exact initialized metadata and generation.
        let epoch = unsafe { self.core.block.metadata.add(self.index.get()).as_ref() }.generation;
        slab::Handle::new(self.index, epoch)
    }
    pub(in crate::depot) fn state_mut(&mut self) -> &mut crate::depot::raw::IndexedState {
        // SAFETY: the admitted metadata cell excludes the dangling empty allocation.
        unsafe { self.core.block.state_mut() }
    }
    pub(in crate::depot) fn release<const RETIRE: bool>(&mut self) {
        let index = self.index;
        let block = &mut self.core.block;
        // SAFETY: this slot and IndexedBlock's layout retain initialized metadata
        // and allocator state in disjoint regions of the same owned allocation.
        let meta = unsafe { block.metadata.add(index.get()).as_mut() };
        if RETIRE {
            match meta.generation.checked_add(1) {
                Some(epoch) => meta.generation = epoch,
                None => {
                    meta.hook = H::new(index, Some(index));
                    return;
                }
            }
        }
        let state = unsafe { block.state_mut() };
        let next = state.free;
        meta.hook = H::new(
            index,
            (next != links::Index::END).then(|| unsafe { links::Index::new_raw(next) }),
        );
        state.free = index.raw();
        state.available += 1;
    }
    pub(in crate::depot) fn into_phase<Q: phase::State>(
        self,
        phase: Q,
    ) -> Slot<'a, T, Tag, MAX, H, N, C, Q> {
        let (core, index) = self.into_owner();
        Slot { core, index, phase }
    }
    pub(in crate::depot) fn into_owner(
        self,
    ) -> (&'a mut slab::Slab<T, Tag, MAX, H, N, C>, links::Index) {
        let this = mem::ManuallyDrop::new(self);
        // SAFETY: callers consume the phase and retain responsibility for its initialized slot.
        (unsafe { ptr::read(&this.core) }, this.index)
    }
}

impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> Slot<'a, T, Tag, MAX, H, N, C>
{
    pub fn into_mut(self) -> &'a mut T {
        // SAFETY: this consuming transition retains the original exclusive source lifetime.
        unsafe { (&mut *self.value()).assume_init_mut() }
    }
}

impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> Slot<'a, T, Tag, MAX, H, N, C, phase::Detached>
{
    pub(super) fn read(self) -> T {
        let (value, spare) = self.into_spare();
        spare.cancel();
        value
    }
    pub(super) fn into_spare(mut self) -> (T, Vacant<'a, T, Tag, MAX, H, N, C, phase::Vacant>) {
        // SAFETY: the consumed detached phase owns one initialized value.
        let value = unsafe { (&*self.value()).assume_init_read() };
        self.state_mut().len -= 1;
        (
            value,
            slab::Vacant {
                entry: self.into_phase(phase::Vacant),
            },
        )
    }
    pub(in crate::depot::queue) fn discard(mut self) {
        unsafe { (&mut *self.value()).assume_init_drop() };
        self.state_mut().len -= 1;
        self.release::<true>();
    }
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count>
    Slot<'a, T, Tag, MAX, links::Hook, 0, C>
{
    pub(in crate::depot::queue) fn detach(
        self,
    ) -> Slot<'a, T, Tag, MAX, links::Hook, 0, C, phase::Detached> {
        unsafe {
            self.core.chain.unlink(&self.core.block, self.index).idle();
        }
        self.into_phase(phase::Detached)
    }
    pub fn push_back(self) {
        if self.core.chain.last() != Some(self.index) {
            unsafe {
                self.core
                    .chain
                    .move_back(&self.core.chain, &self.core.block, self.index)
            };
        }
    }
    pub fn remove(self) -> T {
        self.detach().read()
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Slot<'a, T, Tag, MAX, tagged::Hook, N, C>
{
    /// Appends this initialized slot to the destination without retiring its key.
    pub fn move_back(self, lane: Index<N>) {
        // SAFETY: the occupied slot retains this owner's exact live membership.
        unsafe { tagged::Linked::new(self) }
            .unlink()
            .select(lane)
            .push::<false>();
    }

    pub fn take(
        self,
        lane: Index<N>,
    ) -> (
        T,
        Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Restore<N>>,
    ) {
        let detached = unsafe { tagged::Linked::new(self) }
            .unlink()
            .into_inner()
            .into_phase(phase::Detached);
        let (value, slot) = detached.into_spare();
        (value, slot.into_phase(phase::Restore(lane)))
    }
    pub fn remove(self) -> T {
        unsafe { tagged::Linked::new(self) }
            .unlink()
            .into_inner()
            .into_phase(phase::Detached)
            .read()
    }
}

impl<
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
    P: phase::State,
> Drop for Slot<'_, T, Tag, MAX, H, N, C, P>
{
    fn drop(&mut self) {
        if P::RETIRE {
            self.release::<true>();
        } else if P::RELEASE {
            self.release::<false>();
        }
    }
}

impl<
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
    P: phase::State,
> ops::Deref for Slot<'_, T, Tag, MAX, H, N, C, P>
{
    type Target = slab::Slab<T, Tag, MAX, H, N, C>;
    fn deref(&self) -> &Self::Target {
        self.core
    }
}

pub(in crate::depot) enum Splice<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count> {
    Root(Vacant<'a, T, Tag, MAX, links::Hook, 0, C, phase::Vacant>),
    Parent(
        Live<Vacant<'a, T, Tag, MAX, links::Hook, 0, C, phase::Vacant>, links::Index, links::Hook>,
    ),
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count> Splice<'a, T, Tag, MAX, C> {
    pub(in crate::depot) fn insert(
        self,
        build: impl FnOnce(Option<&mut T>, slab::Handle<Tag, MAX>) -> T,
    ) -> Slot<'a, T, Tag, MAX, links::Hook, 0, C> {
        match self {
            Self::Root(slot) => {
                let key = slot.key();
                slot.insert(build(None, key))
            }
            Self::Parent(parent) => {
                let (slot, index) = parent.into_inner();
                let key = slot.key();
                let parent = Slot {
                    core: &mut *slot.entry.core,
                    index,
                    phase: phase::Occupied,
                }
                .into_mut();
                let value = build(Some(parent), key);
                slot.insert(value)
            }
        }
    }
    pub(in crate::depot) fn remove<R>(
        self,
        value: T,
        unlink: impl FnOnce(Option<&mut T>, T) -> R,
    ) -> R {
        match self {
            Self::Root(slot) => {
                let result = unlink(None, value);
                slot.cancel();
                result
            }
            Self::Parent(parent) => {
                let (slot, index) = parent.into_inner();
                let parent = Slot {
                    core: &mut *slot.entry.core,
                    index,
                    phase: phase::Occupied,
                }
                .into_mut();
                let result = unlink(Some(parent), value);
                slot.cancel();
                result
            }
        }
    }
}

const _: () = {
    assert!(
        mem::size_of::<Slot<'static, (), (), 1, tagged::Hook, 2>>() == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<
            Vacant<'static, (), (), 1, tagged::Hook, 2, capacity::Count, phase::Restore<2>>,
        >() == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<Slot<'static, (), (), 1, tagged::Hook, 2>>>()
            == 2 * mem::size_of::<usize>()
    );
};

impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> AsRef<T> for Slot<'a, T, Tag, MAX, H, N, C>
{
    fn as_ref(&self) -> &T {
        unsafe { (&*self.value()).assume_init_ref() }
    }
}

impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> AsMut<T> for Slot<'a, T, Tag, MAX, H, N, C>
{
    fn as_mut(&mut self) -> &mut T {
        unsafe { (&mut *self.value()).assume_init_mut() }
    }
}
