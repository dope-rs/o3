use std::marker;

use slab::phase;

use crate::depot::{
    array::Index,
    index::Initialized,
    queue::{indexed::slab, links, tagged},
};

// SAFETY: Slab retains the initialized metadata allocation and exact N-head domain.
unsafe impl<T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    crate::depot::raw::TagStore for slab::Slab<T, Tag, MAX, tagged::Hook, N, C>
{
    type Head = links::Chain;
    type Heads = [links::Chain; N];
    fn heads(&self) -> &Self::Heads {
        self.block.heads()
    }
    fn capacity(&self) -> usize {
        self.block.capacity()
    }
    unsafe fn hook(&self, index: links::Index) -> &tagged::Hook {
        &unsafe { self.block.metadata.add(index.get()).as_ref() }.hook
    }
}

// SAFETY: the live proof transfers the original exclusive owner and initialized slot unchanged.
unsafe impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> crate::depot::raw::Project<links::Index, H> for &'a mut slab::Slab<T, Tag, MAX, H, N, C>
{
    type Output = slab::Slot<'a, T, Tag, MAX, H, N, C>;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        slab::Slot {
            core: self,
            index,
            phase: phase::Occupied,
        }
    }
}

// SAFETY: the initialized payload retains the complete source borrow's lifetime.
unsafe impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> crate::depot::raw::Project<links::Index, H> for &'a slab::Slab<T, Tag, MAX, H, N, C>
{
    type Output = &'a T;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        unsafe { self.block.pair(index) }.1
    }
}

// SAFETY: the phantom Handle retains the typed domain; the live proof
// keeps this compact index initialized in the exact source until projection.
unsafe impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> crate::depot::raw::Project<&Typed<Tag, MAX>> for &'a slab::Slab<T, Tag, MAX, H, N, C>
{
    type Output = &'a T;
    unsafe fn project(self, (index, _): &Typed<Tag, MAX>) -> Self::Output {
        unsafe { crate::depot::raw::Proof::<(_, _), Initialized<H>>::new((self, *index)) }.into()
    }
}

// SAFETY: the typed live proof retains the exact initialized index and exclusive source.
unsafe impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    C: crate::depot::raw::Count,
> crate::depot::raw::Project<&Typed<Tag, MAX>> for &'a mut slab::Slab<T, Tag, MAX, H, N, C>
{
    type Output = slab::Slot<'a, T, Tag, MAX, H, N, C>;
    unsafe fn project(self, (index, _): &Typed<Tag, MAX>) -> Self::Output {
        unsafe { crate::depot::raw::Proof::<(_, _), Initialized<H>>::new((self, *index)) }.into()
    }
}

// SAFETY: the consumed path proof retains both distinct initialized positions
// under their exact allocation's exclusive borrow.
unsafe impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count>
    crate::depot::raw::Project<(links::Index, Option<links::Index>)>
    for &'a mut slab::Slab<T, Tag, MAX, links::Hook, 0, C>
{
    type Output = slab::Cursor<'a, T, Tag, MAX, C>;

    unsafe fn project(self, (index, parent): (links::Index, Option<links::Index>)) -> Self::Output {
        slab::Cursor {
            entry: slab::Slot {
                core: self,
                index,
                phase: phase::Occupied,
            },
            parent,
        }
    }
}

// SAFETY: the typed compact key remains initialized in this exact insertion-only scope.
unsafe impl<'id, T, Tag, const MAX: u32, C: crate::depot::raw::Count>
    crate::depot::raw::Project<&Typed<Tag, MAX>> for &slab::Scope<'_, 'id, T, Tag, MAX, C>
{
    type Output = slab::Key<'id, Tag>;

    unsafe fn project(self, (index, _): &Typed<Tag, MAX>) -> Self::Output {
        slab::Key {
            index: *index,
            brand: marker::PhantomData,
        }
    }
}

// SAFETY: these phases retain initialized payload and metadata under the exclusive allocation borrow.
unsafe impl<
    T,
    Tag,
    const MAX: u32,
    const N: usize,
    C: crate::depot::raw::Count,
    P: phase::Initialized,
> crate::depot::raw::TagEntry for slab::Slot<'_, T, Tag, MAX, tagged::Hook, N, C, P>
{
    type Store = slab::Slab<T, Tag, MAX, tagged::Hook, N, C>;
    fn store(&self) -> &Self::Store {
        self.core
    }
    fn index(&self) -> links::Index {
        self.index
    }
    fn hook(&self) -> &tagged::Hook {
        &unsafe { self.core.block.metadata.add(self.index.get()).as_ref() }.hook
    }
}

// SAFETY: the typed compact key identifies a distinct initialized predecessor
// retained by this exact exclusive reservation.
unsafe impl<'a, T, Tag, const MAX: u32, const N: usize, C: crate::depot::raw::Count>
    crate::depot::raw::Project<&Typed<Tag, MAX>>
    for slab::Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Target<N>>
{
    type Output = crate::depot::raw::Proof<
        (
            slab::Vacant<'a, T, Tag, MAX, tagged::Hook, N, C, phase::Vacant>,
            (Index<N>, links::Index),
        ),
        Initialized<tagged::Hook>,
    >;
    unsafe fn project(self, (previous, _): &Typed<Tag, MAX>) -> Self::Output {
        let lane = self.entry.phase.0;
        unsafe {
            crate::depot::raw::Proof::new((self.into_phase(phase::Vacant), (lane, *previous)))
        }
    }
}

/// A compact initialized slot whose phantom handle fixes its slab domain.
type Typed<Tag, const MAX: u32> = (links::Index, marker::PhantomData<slab::Handle<Tag, MAX>>);
