use std::{marker, pin::Pin};

use cycle::Policy;

use crate::depot::{
    arena::slab::{
        self, entries,
        entries::{Busy, Core, Exclusive, Interior, Occupied},
        external::{self, shared},
        key::cycle,
        lookup,
        storage::{self, Plane as _},
    },
    index::Initialized,
    queue::links,
};

mod storages;
pub(in crate::depot) use storages::{SlabCapacity, SlabLinks, SlabSlot};

impl<T, G: external::Generation, Tag> external::Pinned<T, G, Tag> {
    pub(in crate::depot) fn split(
        self: Pin<&mut Self>,
    ) -> (&mut Option<G>, Pin<&mut external::pinned::Inner<T, G, Tag>>) {
        // SAFETY: inner remains pinned; next is a disjoint, unpinned identity field.
        let owner = unsafe { self.get_unchecked_mut() };
        (&mut owner.next, unsafe {
            Pin::new_unchecked(&mut owner.inner)
        })
    }
}

impl<T, G: external::Generation> external::Stamped<T, G> {
    pub(in crate::depot) fn pinned(self: Pin<&mut Self>) -> Pin<&mut T> {
        // SAFETY: the pinned stamp never moves or replaces its payload before in-place Drop.
        unsafe { self.map_unchecked_mut(|stamp| &mut stamp.value) }
    }
}

// SAFETY: the proof retains this exact pinned owner's initialized slot and shared loan.
unsafe impl<'a, T, G: external::Generation, Tag> crate::depot::raw::Project<u32>
    for &'a external::Pinned<T, G, Tag>
{
    type Output = &'a T;

    unsafe fn project(self, index: u32) -> Self::Output {
        // SAFETY: the underlying pinned owner retains the same initialized physical index.
        &unsafe { (&self.inner).project(index) }.value
    }
}

// SAFETY: the original exclusive pinned loan survives in the projected occupied phase.
unsafe impl<'a, T, G: external::Generation, Tag> crate::depot::raw::Project<u32>
    for Pin<&'a mut external::Pinned<T, G, Tag>>
{
    type Output = external::Anchored<'a, T, G, Tag>;

    unsafe fn project(self, index: u32) -> Self::Output {
        let (_, inner) = self.split();
        // SAFETY: the underlying pinned owner retains this same live slot and exclusive loan.
        unsafe { inner.project(index) }.into()
    }
}

// SAFETY: Live retains initialization, the exact core, its outer owner, and the operation plane.
unsafe impl<'a, T, const MAX: u32, C, P> crate::depot::raw::Project<links::Index, P>
    for (&'a Core<T, MAX, Interior<false>>, C)
{
    type Output = Result<
        crate::depot::raw::Proof<
            ((Occupied<'a, T, MAX, Interior<false>, 1>, C), links::Index),
            Initialized<P>,
        >,
        lookup::Busy,
    >;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        let (core, context) = self;
        // SAFETY: the consumed Live proves initialized storage in this exact core.
        let slot = unsafe { core.slots.as_ref().get_unchecked(index.get()) };
        if matches!(slot.state.get(), slab::State::Busy(())) {
            return Err(lookup::Busy { index: index.raw() });
        }
        // SAFETY: initialization excludes free states; the first borrow check excluded reentry.
        Ok(unsafe { crate::depot::raw::Proof::new(((Occupied { core, slot }, context), index)) })
    }
}

// SAFETY: the exact exclusive source and operation plane survive native slot admission.
unsafe impl<'a, T, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, C, P, S>
    crate::depot::raw::Project<links::Index, P>
    for (&'a Core<T, MAX, Exclusive<RECYCLE>, PARTITIONS, S>, C)
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
{
    type Output = (Busy<'a, T, MAX, Exclusive<RECYCLE>, PARTITIONS, P, S>, C);

    unsafe fn project(self, index: links::Index) -> Self::Output {
        let (core, context) = self;
        // SAFETY: Live selects the exact initialized slot of the retained
        // exclusive allocation loan; Busy keeps that loan and operation plane.
        let slot = unsafe { core.slots.as_ref().get_unchecked(index.get()) };
        (
            Busy {
                core,
                slot,
                plane: marker::PhantomData,
            },
            context,
        )
    }
}

// SAFETY: the live proof retains this exact pinned owner's initialized slot and shared loan.
unsafe impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S>
    crate::depot::raw::Project<u32> for &'a slab::Pinned<T, Tag, MAX, RECYCLE, PARTITIONS, S>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
{
    type Output = &'a T;

    unsafe fn project(self, index: u32) -> Self::Output {
        // SAFETY: a live slot is below the retained capacity, which excludes the sentinel.
        let index = unsafe { links::Index::new_raw(index) };
        // SAFETY: the retained shared owner and initialized index are unchanged by this projection.
        let live =
            unsafe { crate::depot::raw::Proof::<_, Initialized<()>>::new((&self.core, index)) };
        Core::read(live).into_inner().0
    }
}

// SAFETY: the exact exclusive pinned loan remains in Anchored until its consuming transition.
unsafe impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S>
    crate::depot::raw::Project<u32>
    for Pin<&'a mut slab::Pinned<T, Tag, MAX, RECYCLE, PARTITIONS, S>>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
{
    type Output = slab::Anchored<'a, T, Tag, MAX, RECYCLE, PARTITIONS, S>;

    unsafe fn project(self, index: u32) -> Self::Output {
        // SAFETY: a live slot is below the retained capacity, which excludes the sentinel.
        let index = unsafe { links::Index::new_raw(index) };
        let store = self.into_ref().get_ref();
        // SAFETY: the borrowed core retains this same pinned owner and initialized slot.
        let live = unsafe {
            crate::depot::raw::Proof::<_, Initialized<()>>::new(((&store.core, ()), index))
        };
        let (entry, ()) = live.into();
        slab::Anchored {
            entry,
            tag: marker::PhantomData,
        }
    }
}

// SAFETY: the initialized slot, original owner context, and operation plane survive borrow admission.
unsafe impl<'a, T, Tag, const MAX: u32, C, P> crate::depot::raw::Project<links::Index, P>
    for (&'a slab::Cell<T, Tag, MAX, false>, C)
{
    type Output = Result<
        crate::depot::raw::Proof<
            (
                (
                    slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior>, Tag>,
                    C,
                ),
                links::Index,
            ),
            Initialized<P>,
        >,
        lookup::Busy,
    >;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        let (owner, context) = self;
        // SAFETY: Cell retains the exact core that owns the consumed live slot.
        let live = unsafe {
            crate::depot::raw::Proof::<(_, _), Initialized<P>>::new(((&owner.core, context), index))
        };
        Ok(unsafe {
            live.into()?
                .map(|((entry, context), index)| ((slab::Borrowed::new(entry), context), index))
        })
    }
}

// SAFETY: the allocated index retains this storage's exact physical extent.
unsafe impl<'a, T, G: external::Generation, Tag> crate::depot::raw::Project<u32, entries::Allocated>
    for &'a shared::Storage<T, G, Tag>
{
    type Output = crate::depot::raw::Proof<
        (&'a shared::Storage<T, G, Tag>, u32),
        Initialized<entries::Allocated>,
    >;

    unsafe fn project(self, index: u32) -> Self::Output {
        unsafe { crate::depot::raw::Proof::new((self, index)) }
    }
}
