use std::{marker, ops};

use planes::map::{State, Table};

use crate::depot::{
    map,
    queue::{
        indexed::planes::{
            self,
            map::{Hooks, Record, Slot},
            phase,
        },
        links,
    },
};

/// Installed payload, physical position, and directory membership in one owner.
pub struct Occupied<'a, K, V, const N: usize, P = ()> {
    pub(in crate::depot::queue::indexed::planes::map) owner: &'a mut Table<K, V, N>,
    pub(in crate::depot::queue::indexed::planes::map) index: links::Index,
    phase: marker::PhantomData<P>,
}

impl<'a, K, V, const N: usize, P> Occupied<'a, K, V, N, P> {
    pub(in crate::depot::queue::indexed::planes::map) fn new(
        owner: &'a mut Table<K, V, N>,
        index: links::Index,
    ) -> Self {
        Self {
            owner,
            index,
            phase: marker::PhantomData,
        }
    }
    fn record(&self) -> &Record<K, V> {
        // SAFETY: Occupied retains this owner's installed record and its complete loan.
        unsafe { self.owner.record(self.index) }
    }

    pub fn key(&self) -> &K {
        &self.record().key
    }
}

impl<K, V, const N: usize> Occupied<'_, K, V, N> {
    fn parts_mut(&mut self) -> (&mut Record<K, V>, Slot<'_, K, V, N>) {
        let owner = &*self.owner;
        let slot = Slot::new(owner, self.index);
        // SAFETY: the exclusive owner lends this installed record; Slot exposes only disjoint hooks.
        let record = unsafe { (*owner.node(self.index).record.get()).assume_init_mut() };
        (record, slot)
    }

    pub fn plane<const P: usize>(&mut self) -> State<'_, K, V, N, P> {
        State::new(Slot::new(self.owner, self.index))
    }

    pub fn split<const P: usize>(&mut self) -> (&mut V, State<'_, K, V, N, P>) {
        let (record, slot) = self.parts_mut();
        (&mut record.value, State::new(slot))
    }

    pub fn remove(self) -> (K, V) {
        let bucket = self.record().bucket;
        // SAFETY: removal consumes the sole record loan and its still-live reverse bucket.
        unsafe { map::raw::Hole::retire(&mut *self.owner, bucket) };
        let owner = &*self.owner;
        let node = unsafe { owner.node(self.index) };
        unsafe {
            owner.queues[0]
                .unlink(&Hooks { owner, plane: 0 }, self.index)
                .idle()
        };
        for plane in 1..N {
            if self.index.is_linked(&node.hooks[plane]) {
                unsafe {
                    owner.queues[plane]
                        .unlink(&Hooks { owner, plane }, self.index)
                        .idle()
                };
            }
        }
        let record = unsafe { (*node.record.get()).assume_init_read() };
        (record.key, record.value)
    }
}

impl<K, V, const N: usize, P> ops::Deref for Occupied<'_, K, V, N, P> {
    type Target = V;

    fn deref(&self) -> &V {
        &self.record().value
    }
}

impl<K, V, const N: usize> ops::DerefMut for Occupied<'_, K, V, N> {
    fn deref_mut(&mut self) -> &mut V {
        &mut self.parts_mut().0.value
    }
}

impl<'a, K, V, const N: usize, const P: usize> Occupied<'a, K, V, N, phase::Front<P>> {
    pub fn pop(self) -> Occupied<'a, K, V, N> {
        let owner = &*self.owner;
        let hooks = Hooks { owner, plane: P };
        // SAFETY: this phase retains the exact front and its original plane.
        crate::depot::raw::Removal::commit(unsafe {
            crate::depot::raw::Pop::at(&owner.queues[P], &hooks, self.index)
        })
        .idle();
        Occupied::new(self.owner, self.index)
    }
}
