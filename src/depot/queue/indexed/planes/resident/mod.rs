mod raw;

use std::num::NonZero;

use crate::{
    alloc,
    depot::{
        heap::indexed,
        queue::indexed::planes::{self, bound, groups, phase},
    },
    mem::{self, slot},
};

/// An initialized value admitted by the exact frozen owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Key<'id>(pub(in crate::depot::queue::indexed::planes) bound::ActiveKey<'id>);

impl<'id> Key<'id> {
    pub const fn active(self) -> bound::ActiveKey<'id> {
        self.0
    }
}

/// Freezes value occupancy and metadata while lending payloads and queue hooks.
pub struct Scope<'storage, 'id, T, M, const PLANES: usize, A, const EXCLUDED: usize> {
    pub(in crate::depot::queue::indexed::planes) source:
        bound::Scope<'storage, 'id, slot::Storage<T, M>, PLANES, A>,
}

impl<'storage, 'id, T, M, const PLANES: usize, P: Ord, C: alloc::raw::Extent, const EXCLUDED: usize>
    Scope<'storage, 'id, T, M, PLANES, bound::Active<'storage, P, C>, EXCLUDED>
{
    pub fn resolve(&self, index: usize) -> Option<bound::ActiveKey<'id>> {
        self.source.resolve(index)
    }

    pub fn admit(&self, key: bound::ActiveKey<'id>) -> Option<Key<'id>> {
        self.source.get(key).get()?;
        Some(Key(key))
    }

    pub fn metadata_at(&self, key: bound::ActiveKey<'id>) -> &M {
        self.source.get(key).metadata()
    }

    pub fn metadata(&self, key: Key<'id>) -> &M {
        self.metadata_at(key.0)
    }

    pub fn get(&self, key: Key<'id>) -> &T {
        // SAFETY: this exact scope admitted Some and exposes no lifecycle mutation.
        unsafe { mem::raw::project(self.source.get(key.0)) }
    }

    pub fn get_mut(&mut self, key: Key<'id>) -> &mut T {
        // SAFETY: the key retains this owner's active and occupied phase.
        // The short exclusive scope loan excludes every parent and hook access.
        let node = unsafe { self.source.core.entries.get_unchecked_mut(key.0.index()) };
        unsafe { mem::raw::project_mut(&mut node.value) }
    }

    pub fn entry(&mut self, key: Key<'id>) -> Entry<'_, T, M, PLANES, false, EXCLUDED> {
        Entry {
            entry: self.source.entry(key.0),
        }
    }

    pub fn select(
        &mut self,
        key: Key<'id>,
    ) -> (
        indexed::Entry<'_, P, C>,
        Entry<'_, T, M, PLANES, false, EXCLUDED>,
    ) {
        let (timer, entry) = self.source.select(key.0);
        (timer, Entry { entry })
    }

    /// Consumes both owners and clears the same companion row before terminal access.
    pub fn into_select<R, const N: usize, const MULTIPLIER: usize>(
        self,
        key: Key<'id>,
        side: groups::Side<'storage, 'id, R, C, N, MULTIPLIER>,
    ) -> (
        indexed::Entry<'storage, P, C>,
        Entry<'storage, T, M, PLANES, true, EXCLUDED>,
    )
    where
        C: crate::depot::raw::Count,
    {
        let index = side.clear(key.active());
        let (timer, entry) = self.source.core.select(self.source.index.priority, index);
        (timer, Entry { entry })
    }

    pub fn front<const PLANE: usize>(&self) -> Option<Key<'id>> {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        let index = self.source.core.queues[PLANE].first()?;
        self.admit(self.resolve(index.get())?)
    }

    pub fn front_entry<const PLANE: usize>(
        &mut self,
    ) -> Option<(
        Key<'id>,
        Entry<'_, T, M, PLANES, false, EXCLUDED, phase::Front<PLANE>>,
    )> {
        let key = self.front::<PLANE>()?;
        Some((
            key,
            Entry {
                entry: planes::Entry::new(self.source.core, key.0.0.into_inner()),
            },
        ))
    }

    pub fn iter<const PLANE: usize>(&self) -> impl Iterator<Item = (Key<'id>, &T)> + '_ {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        self.source
            .core
            .iter::<PLANE>()
            .filter_map(|(index, storage)| {
                let key = self.resolve(index.get())?;
                Some((Key(key), storage.get()?))
            })
    }

    pub fn is_empty<const PLANE: usize>(&self) -> bool {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        self.source.core.queues[PLANE].is_empty()
    }

    pub fn len_in<const PLANE: usize>(&self) -> usize {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        self.source.core.queues[PLANE].len()
    }
}

/// One initialized value; hooks and parent access share each short payload loan.
pub struct Entry<
    'storage,
    T,
    M,
    const PLANES: usize,
    const TERMINAL: bool = false,
    const EXCLUDED: usize = PLANES,
    P = (),
> {
    entry: planes::Entry<'storage, slot::Storage<T, M>, PLANES, P>,
}

impl<T, M, const PLANES: usize, const TERMINAL: bool, const EXCLUDED: usize, P>
    Entry<'_, T, M, PLANES, TERMINAL, EXCLUDED, P>
{
    pub fn index(&self) -> usize {
        self.entry.index()
    }

    pub fn position(&self) -> NonZero<u32> {
        self.entry.position()
    }

    pub fn get(&self) -> &T {
        // SAFETY: construction retains this exact owner's occupied phase.
        unsafe { mem::raw::project(&self.entry) }
    }

    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: this short exclusive loan excludes every hook or parent projection.
        unsafe { mem::raw::project_mut(&mut self.entry) }
    }

    pub fn metadata(&self) -> &M {
        self.entry.metadata()
    }

    pub fn is_linked<const PLANE: usize>(&self) -> bool {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        self.entry.is_linked::<PLANE>()
    }
}

impl<T, M, const PLANES: usize, const TERMINAL: bool, const EXCLUDED: usize>
    Entry<'_, T, M, PLANES, TERMINAL, EXCLUDED>
{
    pub fn link_back<const PLANE: usize>(&mut self) {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        self.entry.plane::<PLANE>().link_back();
    }

    pub fn unlink<const PLANE: usize>(&mut self) {
        const { assert!(PLANE < PLANES && PLANE != EXCLUDED) };
        self.entry.plane::<PLANE>().unlink();
    }
}

impl<
    'storage,
    T,
    M,
    const PLANES: usize,
    const TERMINAL: bool,
    const EXCLUDED: usize,
    const PLANE: usize,
> Entry<'storage, T, M, PLANES, TERMINAL, EXCLUDED, phase::Front<PLANE>>
{
    pub fn pop(self) -> Entry<'storage, T, M, PLANES, TERMINAL, EXCLUDED> {
        Entry {
            entry: self.entry.pop(),
        }
    }
}

impl<'storage, T, M, const PLANES: usize, const EXCLUDED: usize>
    Entry<'storage, T, M, PLANES, true, EXCLUDED>
{
    pub fn take(mut self) -> (T, planes::Entry<'storage, slot::Storage<T, M>, PLANES>) {
        // SAFETY: only consuming the complete frozen scope creates this terminal phase.
        // No old key can regain a scope or keep any payload/hook loan alive.
        let (value, _) = unsafe { mem::raw::occupy(&mut self.entry) }.take();
        (value, self.entry)
    }
}
