use std::{marker, ptr};

use crate::depot::{capacity, map::direct};

/// A direct-address slot validated vacant for one exact key.
#[must_use = "a vacant entry does nothing unless inserted"]
pub struct Vacant<'a, K: crate::depot::raw::Key, V, P = ()> {
    pub(in crate::depot::map::direct) table: &'a mut direct::Table<K, V>,
    pub(in crate::depot::map::direct) key: K,
    pub(in crate::depot::map::direct) plane: marker::PhantomData<fn(P) -> P>,
}

impl<'a, K: crate::depot::raw::Key, V, P> Vacant<'a, K, V, P> {
    pub fn key(&self) -> K {
        self.key
    }

    pub fn table(&self) -> &direct::Table<K, V> {
        self.table
    }

    pub fn insert(self, value: V) {
        // SAFETY: this affine vacancy owns an uninitialized slot, so overwriting
        // its vacant record cannot discard a live value or run drop glue.
        unsafe {
            ptr::write(
                self.table.block.get_unchecked_mut(self.key.index()),
                direct::raw::Entry::new(self.key, value),
            );
        }
        let len = self.table.len() + 1;
        *self.table.block.state_mut() = capacity::Count::new(len as u32);
    }
}

impl<'a, K: crate::depot::raw::Key, V> Vacant<'a, K, V> {
    /// Initializes this slot and retains exclusive access to its exact key.
    pub fn occupy(self, value: V) -> direct::Occupied<'a, K, V> {
        Vacant::<K, V> {
            table: &mut *self.table,
            key: self.key,
            plane: marker::PhantomData,
        }
        .insert(value);
        direct::Occupied {
            table: self.table,
            key: self.key,
            plane: marker::PhantomData,
        }
    }
}
