use std::{marker, mem};

use crate::depot::{
    index::{Identity, Stored},
    map::direct,
    queue::links,
};

/// One direct-address slot whose key records whether its value is initialized.
#[repr(C)]
pub(in crate::depot) struct Entry<K: crate::depot::raw::Key, V> {
    pub(in crate::depot) key: K::Raw,
    pub(in crate::depot) value: mem::MaybeUninit<V>,
    domain: Identity<K>,
}

impl<K: crate::depot::raw::Key, V> Entry<K, V> {
    pub(in crate::depot) fn new(key: K, value: V) -> Self {
        Self {
            key: key.into_raw(),
            value: mem::MaybeUninit::new(value),
            domain: marker::PhantomData,
        }
    }

    pub(in crate::depot) fn vacant() -> Self {
        Self {
            key: K::VACANT,
            value: mem::MaybeUninit::uninit(),
            domain: marker::PhantomData,
        }
    }

    pub(in crate::depot) fn read(&self) -> Option<(K, &V)> {
        (self.key != K::VACANT).then(|| unsafe { self.live() })
    }

    /// # Safety
    /// This slot retains its emitted key and initialized value for the loan.
    pub(in crate::depot) unsafe fn live(&self) -> (K, &V) {
        let key = K::from_raw(unsafe { Stored::new(self.key) });
        (key, unsafe { self.value.assume_init_ref() })
    }

    /// # Safety
    /// `index` names an occupied slot in this exact table.
    pub(in crate::depot) unsafe fn occupied(
        table: &mut direct::Table<K, V>,
        index: usize,
    ) -> direct::Occupied<'_, K, V> {
        let index = unsafe { links::Index::new_usize(index) };
        direct::Occupied::from_live(unsafe { crate::depot::raw::Proof::new(((table, ()), index)) })
            .0
    }
}

impl<K: crate::depot::raw::Key, V> Drop for Entry<K, V> {
    fn drop(&mut self) {
        if self.key == K::VACANT {
            return;
        }
        // SAFETY: this slot's occupied key retains its one initialized value.
        unsafe { self.value.assume_init_drop() };
    }
}
