//! Fixed direct-address maps.

use std::{marker, mem};

pub use error::Error;
pub use occupied::Occupied;
pub use vacant::Vacant;

use crate::{
    alloc::array,
    depot::{capacity, index::Initialized, map::direct, queue::links},
};

mod error;

mod occupied;
pub(in crate::depot) mod raw;
mod vacant;

/// One-allocation direct table for caller-owned exact keys.
pub struct Table<K: crate::depot::raw::Key, V> {
    pub(in crate::depot) block: array::Block<raw::Entry<K, V>, capacity::Count, capacity::Count>,
}

impl<K: crate::depot::raw::Key, V> Table<K, V> {
    /// Allocates and initializes one exact backing table.
    pub fn try_new(capacity: capacity::Count) -> Result<Self, crate::alloc::Error> {
        let block = array::Block::try_new(capacity, capacity::Count::EMPTY)?
            .repeat_with(raw::Entry::vacant);
        Ok(Self { block })
    }

    pub const fn capacity(&self) -> usize {
        self.block.extent().get()
    }

    pub const fn len(&self) -> usize {
        self.block.state().get()
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K: crate::depot::raw::Key, V> Table<K, V> {
    /// # Safety
    /// `index` is occupied in this exact table for the retained borrow.
    pub(in crate::depot) unsafe fn value(&self, index: links::Index) -> &V {
        unsafe {
            self.block
                .get_unchecked(index.get())
                .value
                .assume_init_ref()
        }
    }

    /// # Safety
    /// `index` is occupied in this exact table for the retained exclusive borrow.
    pub(in crate::depot) unsafe fn vacate(
        &mut self,
        index: links::Index,
    ) -> &mut mem::MaybeUninit<V> {
        let (len, entries) = self.block.parts_mut();
        // SAFETY: the caller retains this table's exact occupied index.
        let slot = unsafe { entries.get_unchecked_mut(index.get()) };
        *len = capacity::Count::new((len.get() - 1) as u32);
        slot.key = crate::depot::raw::Slot::vacant();
        &mut slot.value
    }

    pub fn get(&self, key: K) -> Option<&V> {
        let slot = self.block.get(key.index())?;
        (slot.key == crate::depot::raw::Slot::from_key(key))
            .then(|| unsafe { slot.value.assume_init_ref() })
    }

    pub fn entry_at(&self, index: usize) -> Option<(K, &V)> {
        self.block.get(index)?.read()
    }

    pub fn key_at(&self, index: usize) -> Option<K> {
        self.block.get(index)?.key.present().map(K::from_live)
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> {
        self.block.iter().filter_map(raw::Entry::read)
    }

    /// Preserves the original value, complete identity, and operation plane.
    pub fn project<P>(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<P>>,
    ) -> crate::depot::raw::Proof<(&V, K), Initialized<P>> {
        unsafe {
            live.map(|(read, index)| {
                let slot = read.block.get_unchecked(index.get());
                let key = slot.key.live();
                (slot.value.assume_init_ref(), K::from_live(key))
            })
        }
    }
}

impl<K: crate::depot::raw::Key, V> Table<K, V> {
    pub fn entry(&mut self, key: K) -> Option<direct::Occupied<'_, K, V>> {
        let slot = self.block.get(key.index())?;
        (slot.key == crate::depot::raw::Slot::from_key(key)).then_some(direct::Occupied {
            table: self,
            key,
            plane: marker::PhantomData,
        })
    }

    pub fn try_vacant(&mut self, key: K) -> Result<direct::Vacant<'_, K, V>, direct::Error<K>> {
        let index = key.index();
        let slot = self.block.get(index).ok_or(direct::Error::Outside {
            key,
            index,
            capacity: self.capacity(),
        })?;
        if !slot.key.is_vacant() {
            // SAFETY: initialized table slots contain only stored keys or the vacancy sentinel.
            let stored = unsafe { slot.key.live() };
            return Err(direct::Error::Occupied {
                key,
                stored: K::from_live(stored),
            });
        }
        Ok(direct::Vacant {
            table: self,
            key,
            plane: marker::PhantomData,
        })
    }

    pub fn vacant(&mut self, key: K) -> Option<direct::Vacant<'_, K, V>> {
        let slot = self.block.get(key.index())?;
        slot.key.is_vacant().then_some(direct::Vacant {
            table: self,
            key,
            plane: marker::PhantomData,
        })
    }

    pub fn clear(&mut self) {
        if self.is_empty() {
            return;
        }
        for index in 0..self.capacity() {
            // SAFETY: this index belongs to the unchanged backing extent.
            if unsafe { self.block.get_unchecked(index) }.key.is_vacant() {
                continue;
            }
            // SAFETY: the live slot retains this exact native index until removal.
            unsafe {
                self.vacate(links::Index::new_usize(index))
                    .assume_init_drop();
            }
        }
    }
}
