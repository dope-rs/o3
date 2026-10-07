pub(in crate::depot::queue::indexed::planes::map) use place::Place;
use planes::map::{Occupied, Table};

use crate::depot::{
    map,
    queue::{
        indexed::{
            planes,
            planes::map::{Hooks, Record},
        },
        links,
    },
};

mod place;

/// Exclusively reserves one absent key and its vacant physical slot.
pub struct Vacancy<'a, K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) owner: &'a mut Table<K, V, N>,
    pub(in crate::depot::queue::indexed::planes::map) index: links::Index,
    pub(in crate::depot::queue::indexed::planes::map) bucket: map::raw::Vacant,
    pub(in crate::depot::queue::indexed::planes::map) hash: u64,
    pub(in crate::depot::queue::indexed::planes::map) key: K,
}

impl<'a, K: Copy, V, const N: usize> From<map::Vacancy<Place<'a, K, V, N>, u64>>
    for Vacancy<'a, K, V, N>
{
    fn from(entry: map::Vacancy<Place<'a, K, V, N>, u64>) -> Self {
        let (place, bucket, hash) = entry.into_parts();
        Self {
            owner: place.owner,
            index: place.index,
            key: place.key,
            bucket,
            hash,
        }
    }
}

impl<'a, K, V, const N: usize> Vacancy<'a, K, V, N> {
    pub fn insert(self, value: V) -> Occupied<'a, K, V, N> {
        let bucket = unsafe { map::raw::Occupied::new(self.bucket.index()) };
        unsafe {
            self.owner
                .directory
                .insert(self.bucket, self.hash, self.index)
        };
        let core = &self.owner.core;
        let node = unsafe { core.node(self.index) };
        unsafe {
            (*node.record.get()).write(Record {
                key: self.key,
                value,
                bucket,
            })
        };
        unsafe { core.queues[0].link_back(&Hooks { core, plane: 0 }, self.index) };
        Occupied::new(self.owner, self.index)
    }
}
