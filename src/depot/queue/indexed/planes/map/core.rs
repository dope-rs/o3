use std::{cell, mem};

use crate::depot::queue::{
    indexed::planes::map::{Record, Table},
    links,
};

pub(in crate::depot::queue::indexed::planes::map) struct Node<K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) record:
        cell::UnsafeCell<mem::MaybeUninit<Record<K, V>>>,
    pub(in crate::depot::queue::indexed::planes::map) hooks: [links::Hook; N],
}

impl<K, V, const N: usize> Table<K, V, N> {
    pub(in crate::depot::queue::indexed::planes::map) unsafe fn node(
        &self,
        index: links::Index,
    ) -> &Node<K, V, N> {
        unsafe { self.nodes.get_unchecked(index.get()) }
    }

    pub(in crate::depot::queue::indexed::planes::map) unsafe fn record(
        &self,
        index: links::Index,
    ) -> &Record<K, V> {
        unsafe { (*self.node(index).record.get()).assume_init_ref() }
    }
}

pub(in crate::depot::queue::indexed::planes::map) struct Hooks<'a, K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) owner: &'a Table<K, V, N>,
    pub(in crate::depot::queue::indexed::planes::map) plane: usize,
}

// SAFETY: construction retains the exact owner and an admitted plane.
unsafe impl<K, V, const N: usize> crate::depot::raw::Hooks for Hooks<'_, K, V, N> {
    unsafe fn hook_raw(&self, index: links::Index) -> &'_ links::Hook {
        unsafe { self.owner.node(index).hooks.get_unchecked(self.plane) }
    }
}
