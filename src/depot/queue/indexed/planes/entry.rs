use std::{marker, num::NonZero, ops};

use crate::depot::queue::{
    indexed::planes::{self, Core, Hooks, Node, State, phase},
    links,
};

/// Exclusive access to one validated payload and all of its queue hooks.
pub struct Entry<'storage, T, const PLANES: usize, P = ()> {
    pub(in crate::depot::queue::indexed::planes) set: &'storage mut Core<T, PLANES>,
    pub(in crate::depot::queue::indexed::planes) index: links::Index,
    pub(in crate::depot::queue::indexed::planes) phase: marker::PhantomData<P>,
}

impl<'a, T, const PLANES: usize, P> Entry<'a, T, PLANES, P> {
    pub(in crate::depot::queue::indexed::planes) fn new(
        set: &'a mut Core<T, PLANES>,
        index: links::Index,
    ) -> Self {
        Self {
            set,
            index,
            phase: marker::PhantomData,
        }
    }
    pub(in crate::depot::queue::indexed::planes) fn into_phase<Q>(self) -> Entry<'a, T, PLANES, Q> {
        Entry::new(self.set, self.index)
    }
    fn node(&self) -> &Node<T, PLANES> {
        // SAFETY: this entry retains its admitted index and the original exclusive set loan.
        unsafe { self.set.entries.get_unchecked(self.index.get()) }
    }

    fn node_mut(&mut self) -> &mut Node<T, PLANES> {
        // SAFETY: the exclusive entry loan protects this same represented node.
        unsafe { self.set.entries.get_unchecked_mut(self.index.get()) }
    }

    /// Retains the nonzero position when transferring an entry identity.
    pub const fn position(&self) -> NonZero<u32> {
        self.index.position()
    }

    pub const fn index(&self) -> usize {
        self.index.get()
    }

    pub fn front<const PLANE: usize>(self) -> Option<Entry<'a, T, PLANES, phase::Front<PLANE>>> {
        const { assert!(PLANE < PLANES) };
        let index = self.set.queues[PLANE].first()?;
        Some(Entry::new(self.set, index))
    }

    /// Returns whether this entry is linked in `PLANE`.
    pub fn is_linked<const PLANE: usize>(&self) -> bool {
        const { assert!(PLANE < PLANES) };
        self.index.is_linked(&self.node().hooks[PLANE])
    }
}

impl<T, const PLANES: usize> Entry<'_, T, PLANES> {
    /// Resolves one plane while retaining exclusive ownership of its state.
    pub fn plane<const PLANE: usize>(&mut self) -> State<'_, T, PLANES, PLANE> {
        State::new(Entry::new(self.set, self.index))
    }
}

impl<T, const PLANES: usize, P> ops::Deref for Entry<'_, T, PLANES, P> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.node().value
    }
}

impl<T, const PLANES: usize, P> ops::DerefMut for Entry<'_, T, PLANES, P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.node_mut().value
    }
}

impl<'storage, T, const PLANES: usize, const PLANE: usize>
    Entry<'storage, T, PLANES, phase::Front<PLANE>>
{
    /// Moves this exact front to its own plane's tail without revalidating membership.
    pub fn move_to_back(self) {
        let hooks = Hooks::<T, PLANES, PLANE>(&self.set.entries);
        let queue = &self.set.queues[PLANE];
        // SAFETY: Front owns the exact set and plane throughout both mutations.
        let vacant = crate::depot::raw::Removal::commit(unsafe {
            crate::depot::raw::Pop::at(queue, &hooks, self.index)
        });

        crate::depot::raw::Insert::commit(unsafe {
            crate::depot::raw::Insert::<u32, false>::from_vacant(queue, &hooks, vacant)
        });
    }

    /// Detaches this exact front while retaining exclusive payload access.
    pub fn pop(self) -> Entry<'storage, T, PLANES> {
        let hooks = Hooks::<T, PLANES, PLANE>(&self.set.entries);
        // SAFETY: this phase retains the exact front and its original plane.
        crate::depot::raw::Removal::commit(unsafe {
            crate::depot::raw::Pop::at(&self.set.queues[PLANE], &hooks, self.index)
        })
        .idle();
        self.into_phase()
    }
}

impl<'a, T, const N: usize, const P: usize> Entry<'a, T, N, phase::Linked<P>> {
    pub fn unlink(self) -> Entry<'a, T, N, phase::Vacant<P>> {
        crate::depot::raw::Membership::<true>::transition(self)
    }
}

impl<'a, T, const N: usize, const P: usize> Entry<'a, T, N, phase::Vacant<P>> {
    pub fn link_back(self) -> Entry<'a, T, N, phase::Linked<P>> {
        crate::depot::raw::Membership::<false>::transition(self)
    }
}

// SAFETY: this native phase retains the complete exclusive set and its linked hook in P.
unsafe impl<'a, T, const N: usize, const P: usize> crate::depot::raw::Membership<true>
    for Entry<'a, T, N, phase::Linked<P>>
{
    type Output = Entry<'a, T, N, phase::Vacant<P>>;

    fn parts(
        &self,
    ) -> (
        &links::Chain<u32>,
        impl crate::depot::raw::Hooks + '_,
        links::Index,
    ) {
        (
            &self.set.queues[P],
            planes::Hooks::<T, N, P>(&self.set.entries),
            self.index,
        )
    }

    unsafe fn finish(self) -> Self::Output {
        self.into_phase()
    }
}

// SAFETY: this native phase retains the complete exclusive set and its detached hook in P.
unsafe impl<'a, T, const N: usize, const P: usize> crate::depot::raw::Membership<false>
    for Entry<'a, T, N, phase::Vacant<P>>
{
    type Output = Entry<'a, T, N, phase::Linked<P>>;

    fn parts(
        &self,
    ) -> (
        &links::Chain<u32>,
        impl crate::depot::raw::Hooks + '_,
        links::Index,
    ) {
        (
            &self.set.queues[P],
            planes::Hooks::<T, N, P>(&self.set.entries),
            self.index,
        )
    }

    unsafe fn finish(self) -> Self::Output {
        self.into_phase()
    }
}
