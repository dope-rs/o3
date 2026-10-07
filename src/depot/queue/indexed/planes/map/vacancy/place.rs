use planes::map::{Error, Table, table::Source};

use crate::depot::{
    map,
    queue::{indexed::planes, links},
};

/// One uninstalled physical slot below the directory's admission limit.
pub(in crate::depot::queue::indexed::planes::map) struct Place<'a, K, V, const N: usize> {
    pub(super) owner: &'a mut Table<K, V, N>,
    pub(super) index: links::Index,
    pub(super) key: K,
}

// SAFETY: reservation retains the same table used by this query's directory projection.
unsafe impl<K: Copy, V, const N: usize> Source<K, V, N> for Place<'_, K, V, N> {
    fn owner(&self) -> &Table<K, V, N> {
        self.owner
    }
}

impl<'a, K: Copy, V, const N: usize> Place<'a, K, V, N> {
    pub(in crate::depot::queue::indexed::planes::map) fn try_new(
        owner: &'a mut Table<K, V, N>,
        index: usize,
        key: K,
    ) -> Result<Self, Error<K>> {
        let Some(node) = owner.core.nodes.get(index) else {
            return Err(Error::Bounds {
                key,
                index,
                capacity: owner.core.nodes.len(),
            });
        };
        let index = unsafe { links::Index::new_usize(index) };
        if index.is_linked(&node.hooks[0]) {
            let stored = unsafe { owner.core.record(index) }.key;
            return Err(Error::Occupied {
                key,
                stored,
                index: index.get(),
            });
        }
        let capacity = owner.shape.capacity().get();
        if owner.core.queues[0].len() == capacity {
            return Err(Error::Full { key, capacity });
        }
        Ok(Self { owner, index, key })
    }
}

// SAFETY: the exclusive table loan keeps every published record initialized.
unsafe impl<K: Copy, V, const N: usize> map::raw::Query for Place<'_, K, V, N> {
    type Payload = links::Index;
    type Value = K;
    type Occupied = Error<K>;

    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.owner.directory
    }

    fn value(entry: map::Found<&Self>) -> &K {
        &Self::record(entry).key
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let stored = Self::record(entry.read()).key;
        let (owner, _) = entry.into_parts();
        Error::Key {
            key: owner.key,
            stored,
            index: owner.index.get(),
        }
    }
}

// SAFETY: admission proves installed < capacity < buckets; the loan prevents insertion.
unsafe impl<K: Copy, V, const N: usize> map::raw::Spare for Place<'_, K, V, N> {}
