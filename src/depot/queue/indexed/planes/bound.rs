use std::{marker, num::NonZero};

use crate::{
    alloc,
    depot::{
        capacity,
        heap::indexed,
        queue::{
            indexed::planes::{self, groups, phase, resident},
            links,
        },
    },
    mem::slot,
};

/// Retains disjoint loans of one payload owner and its auxiliary storage.
pub struct Scope<'storage, 'id, T, const PLANES: usize, A> {
    pub(in crate::depot::queue::indexed::planes) core: &'storage mut planes::Core<T, PLANES>,
    pub(in crate::depot::queue::indexed::planes) index: A,
    pub(in crate::depot::queue::indexed::planes) brand: marker::PhantomData<capacity::Domain<'id>>,
}

impl<'storage, T, const PLANES: usize, A> Scope<'storage, '_, T, PLANES, &'storage mut A> {
    pub fn with<R>(
        source: &'storage mut planes::Set<T, PLANES, A>,
        run: impl for<'id> FnOnce(Scope<'storage, 'id, T, PLANES, &'storage mut A>) -> R,
    ) -> R {
        run(Scope {
            core: &mut source.core,
            index: &mut source.index,
            brand: marker::PhantomData,
        })
    }
}

/// One current-prefix row; physical keys cannot construct this identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ActiveKey<'id>(
    pub(in crate::depot::queue::indexed::planes) capacity::Key<'id, links::Index>,
);

impl ActiveKey<'_> {
    pub fn index(self) -> usize {
        self.0.as_inner().get()
    }

    pub fn position(self) -> NonZero<u32> {
        self.0.as_inner().position()
    }
}

/// The original heap and the current prefix, split from their coupled owner.
pub struct Active<'storage, P: Ord, C: alloc::raw::Extent> {
    pub(in crate::depot::queue::indexed::planes) priority: &'storage mut indexed::Min<P, C>,
    pub(in crate::depot::queue::indexed::planes) capacity: C,
}

impl<'storage, 'id, T, P: Ord, const PLANES: usize, C: alloc::raw::Extent>
    Scope<'storage, 'id, T, PLANES, Active<'storage, P, C>>
{
    pub fn limit(&self) -> C {
        self.index.capacity
    }

    pub fn resolve(&self, index: usize) -> Option<ActiveKey<'id>> {
        if index >= self.index.capacity.len() {
            return None;
        }
        // SAFETY: construction/growth retains active <= physical and excludes the sentinel.
        let index = unsafe { links::Index::new_usize(index) };
        Some(ActiveKey(capacity::Key::new(unsafe {
            crate::depot::raw::Proof::new(index)
        })))
    }

    pub fn front_entry<const PLANE: usize>(
        &mut self,
    ) -> Option<(
        ActiveKey<'id>,
        planes::Entry<'_, T, PLANES, phase::Front<PLANE>>,
    )> {
        const { assert!(PLANE < PLANES) };
        let index = self.core.queues[PLANE].first()?;
        let key = self.resolve(index.get())?;
        Some((key, planes::Entry::new(self.core, index)))
    }

    pub fn get(&self, key: ActiveKey<'id>) -> &T {
        // SAFETY: the active key retains this exact represented row.
        &unsafe { self.core.entries.get_unchecked(key.index()) }.value
    }

    pub fn entry(&mut self, key: ActiveKey<'id>) -> planes::Entry<'_, T, PLANES> {
        planes::Entry::new(self.core, key.0.into_inner())
    }

    pub fn select(
        &mut self,
        key: ActiveKey<'id>,
    ) -> (indexed::Entry<'_, P, C>, planes::Entry<'_, T, PLANES>) {
        self.core.select(self.index.priority, key.0.into_inner())
    }
}

impl<'storage, 'id, T, M, const PLANES: usize, P: Ord, C: alloc::raw::Extent>
    Scope<'storage, 'id, slot::Storage<T, M>, PLANES, Active<'storage, P, C>>
{
    pub fn resident<const EXCLUDED: usize>(
        self,
    ) -> resident::Scope<'storage, 'id, T, M, PLANES, Active<'storage, P, C>, EXCLUDED> {
        resident::Scope { source: self }
    }

    pub fn install<const EXCLUDED: usize>(
        self,
        key: ActiveKey<'id>,
        value: T,
    ) -> (
        resident::Scope<'storage, 'id, T, M, PLANES, Active<'storage, P, C>, EXCLUDED>,
        resident::Key<'id>,
    ) {
        // SAFETY: this active key retains the exact row; no resident phase exists yet.
        drop(
            unsafe { self.core.entries.get_unchecked_mut(key.index()) }
                .value
                .replace(value),
        );
        (self.resident(), resident::Key(key))
    }
}

impl<
    'storage,
    'id,
    T,
    P: Ord,
    C: crate::depot::raw::Count + alloc::raw::Extent,
    R,
    const PLANES: usize,
    const N: usize,
    const MULTIPLIER: usize,
> Scope<'storage, 'id, T, PLANES, &'storage mut groups::Index<P, C, R, N, MULTIPLIER>>
{
    pub fn split(
        self,
    ) -> (
        Scope<'storage, 'id, T, PLANES, Active<'storage, P, C>>,
        groups::Side<'storage, 'id, R, C, N, MULTIPLIER>,
    ) {
        let groups::Index { priority, records } = self.index;
        let capacity = records.capacity().0;
        (
            Scope {
                core: self.core,
                index: Active { priority, capacity },
                brand: marker::PhantomData,
            },
            groups::Side {
                fifo: records,
                brand: marker::PhantomData,
            },
        )
    }
}
