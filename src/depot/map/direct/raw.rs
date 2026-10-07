use std::mem;

use crate::depot::{map::direct, queue::links};

/// One direct-address slot whose key records whether its value is initialized.
#[repr(C)]
pub(in crate::depot) struct Entry<K: crate::depot::raw::Key, V> {
    pub(in crate::depot) key: crate::depot::raw::Slot<K>,
    pub(in crate::depot) value: mem::MaybeUninit<V>,
}

impl<K: crate::depot::raw::Key, V> Entry<K, V> {
    pub(in crate::depot) fn new(key: K, value: V) -> Self {
        Self {
            key: crate::depot::raw::Slot::from_key(key),
            value: mem::MaybeUninit::new(value),
        }
    }

    pub(in crate::depot) fn vacant() -> Self {
        Self {
            key: crate::depot::raw::Slot::vacant(),
            value: mem::MaybeUninit::uninit(),
        }
    }

    pub(in crate::depot) fn read(&self) -> Option<(K, &V)> {
        let key = K::from_live(self.key.present()?);
        Some((key, unsafe { self.value.assume_init_ref() }))
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
        if self.key.is_vacant() {
            return;
        }
        // SAFETY: this slot's occupied key retains its one initialized value.
        unsafe { self.value.assume_init_drop() };
    }
}
