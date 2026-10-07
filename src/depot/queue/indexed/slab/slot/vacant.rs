use std::{mem, ops};

use slab::{Slot, phase};

use crate::depot::{
    array::Index,
    capacity,
    queue::{indexed::slab, links, tagged},
    raw::Live,
};

/// One inactive reservation retaining its generation and cancellation policy.
#[must_use]
pub struct Vacant<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    H: crate::depot::raw::IndexedHook = links::Hook,
    const N: usize = 0,
    C: crate::depot::raw::Count = capacity::Count,
    P: phase::State = phase::Vacant,
> {
    pub(in crate::depot) entry: Slot<'a, T, Tag, MAX, H, N, C, P>,
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
> Vacant<'a, T, Tag, MAX, H, N, C, P>
{
    pub(in crate::depot) fn key(&self) -> slab::Handle<Tag, MAX> {
        self.entry.key()
    }
    pub(in crate::depot) fn into_phase<Q: phase::State>(
        self,
        phase: Q,
    ) -> Vacant<'a, T, Tag, MAX, H, N, C, Q> {
        Vacant {
            entry: self.entry.into_phase(phase),
        }
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
    P: phase::State,
> ops::Deref for Vacant<'a, T, Tag, MAX, H, N, C, P>
{
    type Target = slab::Slab<T, Tag, MAX, H, N, C>;
    fn deref(&self) -> &Self::Target {
        self.entry.core
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
> Vacant<'a, T, Tag, MAX, H, N, C, phase::Vacant>
{
    fn write(self, value: T) -> Vacant<'a, T, Tag, MAX, H, N, C, phase::Metadata> {
        let mut entry = self.entry.into_phase(phase::Metadata);
        unsafe {
            entry.value().write(mem::MaybeUninit::new(value));
        }
        entry.state_mut().len += 1;
        Vacant { entry }
    }
    pub(in crate::depot) fn cancel(self) {
        self.entry.into_phase(phase::Metadata).release::<true>();
    }
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count>
    Vacant<'a, T, Tag, MAX, links::Hook, 0, C, phase::Vacant>
{
    pub fn fill(self, value: T) {
        self.write(value).link_back();
    }
    pub fn insert(self, value: T) -> Slot<'a, T, Tag, MAX, links::Hook, 0, C> {
        let mut entry = self.write(value);
        entry.link_back();
        entry.entry.into_phase(phase::Occupied)
    }
    pub fn insert_with(
        self,
        build: impl FnOnce(slab::Handle<Tag, MAX>) -> T,
    ) -> Slot<'a, T, Tag, MAX, links::Hook, 0, C> {
        let value = build(self.key());
        self.insert(value)
    }
    pub fn try_insert<E>(
        self,
        build: impl FnOnce(slab::Handle<Tag, MAX>) -> Result<T, E>,
    ) -> Result<Slot<'a, T, Tag, MAX, links::Hook, 0, C>, E> {
        let value = match build(self.key()) {
            Ok(value) => value,
            Err(error) => {
                self.cancel();
                return Err(error);
            }
        };
        Ok(self.insert(value))
    }
}

impl<T, Tag, const MAX: u32, C: crate::depot::raw::Count>
    Vacant<'_, T, Tag, MAX, links::Hook, 0, C, phase::Metadata>
{
    fn link_back(&mut self) {
        // SAFETY: the initialized reservation retains its exact detached slot.
        unsafe {
            self.entry
                .core
                .chain
                .link_back(&self.entry.core.block, self.entry.index)
        };
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Vacant>
{
    pub fn select(
        self,
        lane: Index<N>,
    ) -> Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Target<N>> {
        self.into_phase(phase::Target(lane))
    }
    pub(in crate::depot) fn publish<const FRONT: bool>(
        self,
        value: T,
        lane: Index<N>,
    ) -> Slot<'a, T, Tag, MAX, tagged::Hook, N, C> {
        let mut entry = self.write(value);
        entry.link::<FRONT>(lane);
        entry.entry.into_phase(phase::Occupied)
    }
}

impl<T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Vacant<'_, T, Tag, MAX, tagged::Hook, N, C, phase::Metadata>
{
    fn link<const FRONT: bool>(&mut self, lane: Index<N>) {
        // SAFETY: the initialized reservation retains its exact detached slot.
        let vacant = unsafe { tagged::Vacant::new(&mut self.entry) }.select(lane);
        vacant.push::<FRONT>();
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Target<N>>
{
    pub fn insert(self, value: T) -> Slot<'a, T, Tag, MAX, tagged::Hook, N, C> {
        let lane = self.entry.phase.0;
        self.into_phase(phase::Vacant).publish::<false>(value, lane)
    }
    pub fn append(
        self,
        previous: slab::Handle<Tag, MAX>,
    ) -> Result<Append<'a, T, Tag, MAX, N, C>, slab::Error<Tag, MAX, C>> {
        let lane = self.entry.phase.0;
        let previous = previous.resolve(self.into_phase(phase::Vacant))?;
        // SAFETY: the selected destination and resolved predecessor share this exact reservation.
        Ok(unsafe { previous.map(|(entry, index)| (entry, (lane, index))) })
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Restore<N>>
{
    pub fn restore(self, value: T) {
        let lane = self.entry.phase.0;
        self.into_phase(phase::Vacant)
            .write(value)
            .link::<true>(lane);
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Append<'a, T, Tag, MAX, N, C>
{
    pub fn insert(self, value: T) -> (slab::Handle<Tag, MAX>, &'a mut T) {
        let (vacant, (lane, index)) = self.into_inner();
        let entry = vacant.publish::<false>(value, lane);
        let key = entry.key();
        let (core, _) = entry.into_owner();
        let previous = Slot {
            core,
            index,
            phase: phase::Occupied,
        }
        .into_mut();
        (key, previous)
    }
}

/// An append reservation retains its selected lane and exact predecessor.
type Append<'a, T, Tag, const MAX: u32, const N: usize, C> = Live<
    Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Vacant>,
    (Index<N>, links::Index),
    tagged::Hook,
>;
