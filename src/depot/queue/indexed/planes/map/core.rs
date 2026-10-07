use std::{array::from_fn, cell, mem};

use crate::{
    alloc::{self, array},
    depot::{
        capacity,
        queue::{indexed::planes::map::Record, links},
    },
};

pub(in crate::depot::queue::indexed::planes::map) struct Node<K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) record:
        cell::UnsafeCell<mem::MaybeUninit<Record<K, V>>>,
    pub(in crate::depot::queue::indexed::planes::map) hooks: [links::Hook; N],
}

pub(in crate::depot::queue::indexed::planes::map) struct Core<K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) nodes:
        array::Block<Node<K, V, N>, capacity::Count>,
    pub(in crate::depot::queue::indexed::planes::map) queues: [links::Chain<u32>; N],
}

impl<K, V, const N: usize> Core<K, V, N> {
    pub(in crate::depot::queue::indexed::planes::map) fn try_new(
        count: usize,
    ) -> Result<Self, alloc::Error> {
        const { assert!(N > 0) };
        let count = capacity::Count::try_count(count, alloc::Error::Overflow)?;
        let nodes = array::Block::try_new(count, ())?.init(|index| {
            // SAFETY: the retained count excludes the compact sentinel.
            let index = unsafe { links::Index::new_usize(index) };
            Node {
                record: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
                hooks: from_fn(|_| links::Hook::new(index)),
            }
        });
        Ok(Self {
            nodes,
            queues: [const { links::Chain::new() }; N],
        })
    }

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

impl<K, V, const N: usize> Drop for Core<K, V, N> {
    fn drop(&mut self) {
        if !mem::needs_drop::<Record<K, V>>() {
            return;
        }
        let mut next = self.queues[0].first();
        while let Some(index) = next {
            let node = unsafe { self.node(index) };
            next = node.hooks[0].next_index();
            unsafe { (*node.record.get()).assume_init_drop() };
        }
    }
}

pub(in crate::depot::queue::indexed::planes::map) struct Hooks<'a, K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) core: &'a Core<K, V, N>,
    pub(in crate::depot::queue::indexed::planes::map) plane: usize,
}

// SAFETY: construction retains the exact core and an admitted plane.
unsafe impl<K, V, const N: usize> crate::depot::raw::Hooks for Hooks<'_, K, V, N> {
    unsafe fn hook_raw(&self, index: links::Index) -> &'_ links::Hook {
        unsafe { self.core.node(index).hooks.get_unchecked(self.plane) }
    }
}
