use std::ops;

use crate::depot::map;

/// An installed key and payload, borrowed from their directory owner.
pub struct Record<K, V> {
    pub(in crate::depot::queue::indexed::planes::map) key: K,
    pub(in crate::depot::queue::indexed::planes::map) value: V,
    pub(in crate::depot::queue::indexed::planes::map) bucket: map::raw::Occupied,
}

impl<K, V> Record<K, V> {
    pub fn key(&self) -> &K {
        &self.key
    }
}

impl<K, V> ops::Deref for Record<K, V> {
    type Target = V;

    fn deref(&self) -> &V {
        &self.value
    }
}
