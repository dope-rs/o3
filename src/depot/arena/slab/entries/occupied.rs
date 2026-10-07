use std::marker;

use cycle::Policy;

use crate::depot::{
    arena::slab::{
        self, entries,
        key::{self, cycle},
        lookup,
        storage::{self, Plane},
    },
    capacity,
    capacity::Physical,
};

pub(in crate::depot) struct Occupied<
    'a,
    T,
    const MAX: u32,
    M: entries::Mode,
    const PARTITIONS: usize,
    S = storage::Heap,
    I = storage::Eager,
> where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
    pub(in crate::depot) slot: &'a crate::depot::raw::SlabSlot<
        T,
        key::Generation<MAX>,
        M::Busy,
        <M::Cycle as Policy>::Retired,
    >,
}

impl<'a, T, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S, I>
    Occupied<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn read(self) -> &'a T {
        // SAFETY: the admitted slot retains its exclusive owner's shared loan.
        unsafe { self.slot.value() }
    }
}

impl<'a, T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, S>
    Occupied<'a, T, MAX, M, PARTITIONS, S>
where
    S: storage::Storage,
{
    pub(in crate::depot) fn try_at(
        core: &'a entries::Core<T, MAX, M, PARTITIONS, S>,
        index: u32,
    ) -> lookup::Result<Self, M> {
        let slots = &core.slots;
        if index as usize >= slots.capacity() {
            return Err(lookup::Error::Index {
                index,
                capacity: slots.count(),
            });
        }
        // SAFETY: eager storage initialized every slot of this admitted extent.
        let live = unsafe { crate::depot::raw::Proof::new((core, index)) };
        entries::Core::try_occupied(live)
    }
}

impl<'a, T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, S, I>
    Occupied<'a, T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn at(
        core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
        index: u32,
    ) -> Option<Self> {
        let slot = core.slots.get(index)?;
        if slot.state.get() != slab::State::Occupied {
            return None;
        }
        Some(Self { core, slot })
    }

    pub(in crate::depot) fn key(
        core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
        index: u32,
        epoch: key::Generation<MAX>,
    ) -> Option<Self> {
        let entry = Self::at(core, index)?;
        (entry.slot.generation.get() == epoch).then_some(entry)
    }

    pub(in crate::depot) fn take(self) -> entries::Busy<'a, T, MAX, M, PARTITIONS, (), S, I> {
        if M::REENTRANT {
            self.slot.state.set(M::TAKEN);
        }
        entries::Busy {
            plane: marker::PhantomData,
            core: self.core,
            slot: self.slot,
        }
    }
}

impl<'a, T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, S, I>
    Occupied<'a, T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn try_key<Tag>(
        core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
        key: key::Handle<Tag, MAX>,
    ) -> Lookup<Self, Tag, MAX, M> {
        let slots = &core.slots;
        let slot = slots.get(key.index()).ok_or(key::Error::Index {
            key,
            capacity: slots.count(),
        })?;
        let actual = slot.generation.get();
        match slot.state.get().classify(actual, key.generation()) {
            slab::Admitted::Exact => Ok(Self { core, slot }),
            slab::Admitted::Refused(refusal) => Err(key::Error::refused(key, actual, refusal)),
            slab::Admitted::Borrowed(borrow) => Err(key::Error::Busy {
                key,
                actual,
                borrow,
            }),
        }
    }

    pub(in crate::depot) fn generation(&self) -> key::Generation<MAX> {
        self.slot.generation.get()
    }
}

impl<'a, T, const MAX: u32, S> Occupied<'a, T, MAX, entries::Exclusive<false>, 1, S>
where
    S: storage::Storage,
{
    pub(in crate::depot) fn try_bound<'h, Tag>(
        proof: PhysicalSlot<'a, 'h, T, Tag, MAX, S>,
    ) -> Result<Self, key::Stale<'h, Tag, MAX>> {
        let (core, key) = proof.into_inner();
        let slots = core.slots.as_ref();
        // SAFETY: the retained bound selects this eager owner's initialized extent.
        let slot = unsafe { slots.get_unchecked(key.as_inner().index() as usize) };
        let actual = slot.generation.get();
        match slot
            .state
            .get()
            .classify(actual, key.as_inner().generation())
        {
            slab::Admitted::Exact => Ok(Self { core, slot }),
            slab::Admitted::Refused(refusal) => Err(key::Stale::refused(key, actual, refusal)),
        }
    }
}

type Lookup<S, T, const MAX: u32, M> = Result<
    S,
    key::Error<
        T,
        MAX,
        <M as entries::Mode>::Busy,
        <<M as entries::Mode>::Cycle as Policy>::Retired,
    >,
>;

type PhysicalSlot<'a, 'h, T, Tag, const MAX: u32, S> = crate::depot::raw::Proof<
    (
        &'a entries::Core<T, MAX, entries::Exclusive<false>, 1, S>,
        capacity::Key<'h, key::Handle<Tag, MAX>>,
    ),
    Physical,
>;
