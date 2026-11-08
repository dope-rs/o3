use std::mem;

use slab::{Slot, phase};

use crate::depot::{
    array::Index,
    capacity,
    queue::{indexed::slab, links, tagged},
    raw::Live,
};

/// One inactive reservation retaining its generation and cancellation policy.
pub type Vacant<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    H = links::Hook,
    const N: usize = 0,
    C = capacity::Count,
    P = phase::Vacant,
> = Slot<'a, T, Tag, MAX, H, N, C, P>;

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
        let mut entry = self.into_phase(phase::Metadata);
        unsafe {
            entry.value().write(mem::MaybeUninit::new(value));
        }
        entry.state_mut().len += 1;
        entry
    }
    pub(in crate::depot) fn cancel(self) {
        self.into_phase(phase::Metadata).release::<true>();
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
        entry.into_phase(phase::Occupied)
    }
    pub fn insert_with(
        self,
        build: impl FnOnce(slab::Handle<Tag, MAX>) -> T,
    ) -> Slot<'a, T, Tag, MAX, links::Hook, 0, C> {
        let value = build(self.handle());
        self.insert(value)
    }
    pub fn try_insert<E>(
        self,
        build: impl FnOnce(slab::Handle<Tag, MAX>) -> Result<T, E>,
    ) -> Result<Slot<'a, T, Tag, MAX, links::Hook, 0, C>, E> {
        let value = match build(self.handle()) {
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
        unsafe { self.core.chain.link_back(&self.core.block, self.index) };
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
        let entry = self.write(value);
        // SAFETY: the initialized reservation retains its exact detached slot.
        unsafe { tagged::Vacant::new(entry) }
            .push_at::<FRONT, N>(lane)
            .into_inner()
            .into_phase(phase::Occupied)
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Target<N>>
{
    pub fn insert(self, value: T) -> Slot<'a, T, Tag, MAX, tagged::Hook, N, C> {
        let lane = self.phase.0;
        self.into_phase(phase::Vacant).publish::<false>(value, lane)
    }
    pub fn append(
        self,
        previous: slab::Handle<Tag, MAX>,
    ) -> Result<Append<'a, T, Tag, MAX, N, C>, slab::Error<Tag, MAX, C>> {
        let lane = self.phase.0;
        let previous = previous.resolve(self.into_phase(phase::Vacant))?;
        // SAFETY: the selected destination and resolved predecessor share this exact reservation.
        Ok(unsafe { previous.map(|(entry, index)| (entry, (lane, index))) })
    }
}

impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Restore<N>>
{
    pub fn restore(self, value: T) {
        let lane = self.phase.0;
        drop(self.into_phase(phase::Vacant).publish::<true>(value, lane));
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
