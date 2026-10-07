use std::marker;

use crate::depot::{map::direct, queue::links, raw::Live};

/// A direct-address slot validated occupied for one exact key.
#[must_use = "an occupied entry retains exclusive access to its table slot"]
pub struct Occupied<'a, K: crate::depot::raw::Key, V, P = ()> {
    pub(in crate::depot::map::direct) table: &'a mut direct::Table<K, V>,
    pub(in crate::depot::map::direct) key: K,
    pub(in crate::depot::map::direct) plane: marker::PhantomData<fn(P) -> P>,
}

impl<'a, K: crate::depot::raw::Key, V> Occupied<'a, K, V> {
    pub fn from_live<C, P>(
        live: Live<(&'a mut direct::Table<K, V>, C), links::Index, P>,
    ) -> (Occupied<'a, K, V, P>, C) {
        let ((table, context), index) = live.into_inner();
        let slot = unsafe { table.block.get_unchecked(index.get()) };
        let key = K::from_live(unsafe { slot.key.live() });
        (
            Occupied {
                table,
                key,
                plane: marker::PhantomData,
            },
            context,
        )
    }
}

impl<'a, K: crate::depot::raw::Key, V, P> Occupied<'a, K, V, P> {
    pub fn key(&self) -> K {
        self.key
    }

    pub(in crate::depot) fn index(&self) -> usize {
        self.key.index()
    }

    pub(in crate::depot) fn table(&self) -> &direct::Table<K, V> {
        self.table
    }

    fn slot(&self) -> &direct::raw::Entry<K, V> {
        unsafe { self.table.block.get_unchecked(self.key.index()) }
    }

    fn slot_mut(&mut self) -> &mut direct::raw::Entry<K, V> {
        unsafe { self.table.block.get_unchecked_mut(self.key.index()) }
    }

    fn into_slot(self) -> &'a mut direct::raw::Entry<K, V> {
        unsafe { self.table.block.get_unchecked_mut(self.key.index()) }
    }

    pub fn get(&self) -> &V {
        unsafe { self.slot().value.assume_init_ref() }
    }

    pub(in crate::depot) fn get_mut(&mut self) -> &mut V {
        unsafe { self.slot_mut().value.assume_init_mut() }
    }

    pub fn into_mut(self) -> &'a mut V {
        unsafe { self.into_slot().value.assume_init_mut() }
    }

    pub fn remove(self) -> V {
        self.take().1
    }

    pub fn take(self) -> (direct::Vacant<'a, K, V, P>, V) {
        // SAFETY: this exact occupied key was admitted within its u32-sized table.
        let index = unsafe { links::Index::new_usize(self.index()) };
        unsafe { self.take_at(index) }
    }

    /// # Safety
    /// `index` is this occupied entry's previously resolved exact table position.
    pub(in crate::depot) unsafe fn take_at(
        self,
        index: links::Index,
    ) -> (direct::Vacant<'a, K, V, P>, V) {
        let value = unsafe { self.table.vacate(index).assume_init_read() };
        (
            direct::Vacant {
                table: self.table,
                key: self.key,
                plane: marker::PhantomData,
            },
            value,
        )
    }

    pub fn discard(self) {
        // SAFETY: this exact occupied key was admitted within its u32-sized table.
        let index = unsafe { links::Index::new_usize(self.index()) };
        unsafe { self.discard_at(index) };
    }

    /// # Safety
    /// `index` is this occupied entry's previously resolved exact table position.
    pub(in crate::depot) unsafe fn discard_at(self, index: links::Index) {
        unsafe { self.table.vacate(index).assume_init_drop() };
    }
}
