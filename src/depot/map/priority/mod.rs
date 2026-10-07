use std::{cmp, mem, ptr};

pub(in crate::depot::map::priority) use layout::Layout;
pub use min::Min;
pub use plan::Plan;

use crate::depot::{
    heap::{self, hole},
    map::{
        self,
        priority::store::{Pair, Slot, Store},
    },
    queue::links,
};

pub mod errors;
mod min;
mod plan;

mod layout;
mod node;
pub(in crate::depot::map::priority) use node::Node;
mod store;

pub(in crate::depot::map::priority) struct Core<K, V, P: Ord> {
    table: map::raw::Table<u32>,
    store: Store<K, V, P>,
    indices: ptr::NonNull<u32>,
}

/// A single-probe view of an occupied or vacant scheduled key.
pub enum Entry<'a, K, P: Ord, V> {
    Occupied(Occupied<'a, K, P, V>),
    Vacant(Vacant<'a, K, P, V>),
}

pub struct Occupied<'a, K, P: Ord, V> {
    core: &'a mut Core<K, V, P>,
    bucket: map::raw::Occupied,
    index: u32,
}

/// A vacant entry retaining the exact key that admitted its hash and bucket.
pub struct Vacant<'a, K, P: Ord, V> {
    entry: Insertion<'a, K, P, V>,
    key: K,
}

pub(in crate::depot::map::priority) struct Insertion<'a, K, P: Ord, V> {
    core: &'a mut Core<K, V, P>,
    bucket: map::raw::Vacant,
    slot: Slot,
    hash: u64,
}

impl<K, V, P: Ord> Core<K, V, P> {
    pub(in crate::depot::map::priority) fn try_new(
        plan: Plan<K, P, V>,
    ) -> Result<Self, crate::alloc::Exhausted> {
        let layout = Layout::from_plan(plan);
        let fields = layout.bundle.try_allocate()?;
        let table = unsafe { map::raw::Table::from_fields(fields.prior().prior(), layout.buckets) };
        Ok(Self {
            table,
            store: unsafe { Store::new(fields.prior().tail(), layout.plan) },
            indices: fields.tail(),
        })
    }
}

// SAFETY: occupied buckets own nodes bounded by half the nonempty directory width.
unsafe impl<'a, K, V, P: Ord> map::raw::Query for &'a Core<K, V, P> {
    const SPARE: bool = true;

    type Payload = u32;
    type Value = Pair<K, V, P>;
    type Occupied = &'a V;

    fn table(&self) -> &map::raw::Table<u32> {
        &self.table
    }

    fn value(entry: map::Found<&Self>) -> &Self::Value {
        let (owner, payload) = entry.project();
        let index = *payload;
        unsafe { owner.store.pair(index) }
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        &unsafe { owner.store.pair(index) }.value
    }
}

// SAFETY: store admission preserves the same node and directory capacity bound.
unsafe impl<'a, K, V, P: Ord> map::raw::Query for &'a mut Core<K, V, P> {
    const SPARE: bool = true;

    type Payload = u32;
    type Value = Pair<K, V, P>;
    type Occupied = Occupied<'a, K, P, V>;

    fn table(&self) -> &map::raw::Table<u32> {
        &self.table
    }

    fn value(entry: map::Found<&Self>) -> &Self::Value {
        let (owner, payload) = entry.project();
        let index = *payload;
        unsafe { owner.store.pair(index) }
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let index = *entry.read().payload();
        let (owner, bucket) = entry.into_parts();
        Occupied {
            core: owner,
            bucket,
            index,
        }
    }
}

unsafe impl<'a, K, V, P: Ord> map::raw::Insertion for &'a mut Core<K, V, P> {
    type Vacant = Insertion<'a, K, P, V>;

    fn vacant(entry: map::Vacancy<Self>, hash: u64) -> Option<Self::Vacant> {
        let (owner, bucket, ()) = entry.into_parts();
        let slot = owner.store.vacant()?;
        Some(Insertion {
            core: owner,
            bucket,
            slot,
            hash,
        })
    }
}

unsafe impl<K, V, P: Ord> map::raw::Mutation for &mut Core<K, V, P> {
    unsafe fn table_mut(&mut self) -> &mut map::raw::Table<u32> {
        &mut self.table
    }

    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, hash: u64) {
        let (moved, index) = unsafe { self.table.shift(hole, next, hash) };
        let index = *index;
        unsafe { self.store.set_bucket(index, moved) };
    }
}

impl<'a, K, V, P: Ord> Occupied<'a, K, P, V> {
    /// The compact physical slot retained by this exact live source.
    /// It is not a generational handle and cannot authorize access after removal.
    pub fn index(&self) -> links::Index {
        // SAFETY: the admitted map plan bounds every node below its non-sentinel capacity.
        unsafe { links::Index::new_raw(self.index) }
    }

    pub fn key(&self) -> &K {
        &self.pair().key
    }

    fn pair(&self) -> &Pair<K, V, P> {
        // SAFETY: this occupied capability retains its exact core and initialized node.
        unsafe { self.core.store.pair(self.index) }
    }

    fn pair_mut(&mut self) -> &mut Pair<K, V, P> {
        // SAFETY: this capability exclusively owns its initialized node for the full borrow.
        unsafe { self.core.store.pair_mut(self.index) }
    }

    pub fn get(&self) -> &V {
        &self.pair().value
    }

    pub fn priority(&self) -> &P {
        &self.pair().priority
    }

    pub fn get_mut(&mut self) -> &mut V {
        &mut self.pair_mut().value
    }

    pub fn set(&mut self, priority: P) {
        let (owner, position, index) = self.entry(self.index).into_inner();
        let len = owner.store.len();
        let path = hole::Path::repair(owner, position, len, &(index, &priority), Core::compare);
        // SAFETY: the retained occupied entry owns this exact resident priority.
        let old = mem::replace(
            &mut unsafe { path.owner.store.pair_mut(index) }.priority,
            priority,
        );
        path.apply(index).commit::<true>();
        drop(old);
    }

    fn entry<T>(&mut self, next: T) -> heap::Entry<&mut Core<K, V, P>, T> {
        let position = {
            let nodes = self.core.store.edit().1;
            unsafe {
                nodes
                    .get_unchecked_mut(self.index as usize)
                    .assume_init_mut()
            }
            .position as usize
        };
        unsafe { heap::Entry::new(&mut *self.core, position, next) }
    }

    pub fn remove(self) -> V {
        self.remove_parts().1
    }

    pub(in crate::depot::map::priority) fn remove_parts(mut self) -> (K, V, P) {
        heap::raw::Hole::retire(self.entry(()));
        unsafe { map::raw::Hole::retire(&mut *self.core, self.bucket) };
        let Pair {
            key,
            value,
            priority,
        } = unsafe { self.core.store.remove(self.index) };
        (key, value, priority)
    }
}

impl<'a, K, V, P: Ord> Vacant<'a, K, P, V> {
    /// The exact reserved physical slot, which is not initialized until insertion.
    pub fn index(&self) -> links::Index {
        // SAFETY: store admission bounds the reserved slot in this map's compact plan.
        unsafe { links::Index::new_raw(self.entry.slot.index()) }
    }

    pub fn insert(self, priority: P, value: V) {
        self.entry.insert(self.key, priority, value);
    }
}

impl<K, V, P: Ord> Insertion<'_, K, P, V> {
    fn insert(self, key: K, priority: P, value: V) {
        let pair = Pair {
            key,
            value,
            priority,
        };
        let len = self.core.store.len();
        let path = hole::Path::up(
            self.core,
            len,
            &(self.slot.index(), &pair.priority),
            Core::compare,
        );
        let index = path.owner.store.insert(self.slot, pair, &self.bucket);
        unsafe { path.owner.table.insert(self.bucket, self.hash, index) };
        path.append(index).commit::<true>();
    }
}

impl<K, V, P: Ord> Drop for Core<K, V, P> {
    fn drop(&mut self) {
        let layout = Layout::from_plan(self.store.plan());
        let base = self.table.base();
        self.store.clear();
        unsafe { layout.bundle.deallocate(base) }
    }
}

unsafe impl<K, V, P: Ord> hole::raw::Source for &mut Core<K, V, P> {
    type Value = u32;

    fn len(&self) -> u32 {
        self.store.len() as u32
    }

    unsafe fn len_mut(&mut self) -> &mut u32 {
        self.store.edit().0
    }

    fn compare(entry: hole::At<&Self, &u32>) -> cmp::Ordering {
        let (owner, position, value) = entry.into_inner();
        let priority = &unsafe { owner.store.pair(*value) }.priority;
        Core::compare(hole::At::new(owner, position, &(*value, priority)))
    }

    fn compare_at(left: hole::At<&Self>, right: hole::At<&Self>) -> cmp::Ordering {
        let (owner, left, ()) = left.into_inner();
        let (other, right, ()) = right.into_inner();
        let index = unsafe { owner.indices.as_ptr().add(left).read() };
        let priority = &unsafe { owner.store.pair(index) }.priority;
        Core::compare(hole::At::new(other, right, &(index, priority)))
    }

    fn read<const REMOVE: bool>(entry: hole::At<&mut Self>) -> u32 {
        let (owner, position, ()) = entry.into_inner();
        let index = unsafe { owner.indices.as_ptr().add(position).read() };
        if REMOVE {
            unsafe { owner.reindex(index, u32::MAX) };
        }
        index
    }

    fn shift(step: hole::Step<'_, Self>) {
        let (owner, source, target) = step.into_inner();
        let index = unsafe { owner.indices.as_ptr().add(source).read() };
        unsafe { owner.indices.as_ptr().add(target).write(index) };
        unsafe { owner.reindex(index, target as u32) };
    }

    fn write<const PLACE: bool>(finish: hole::Finish<Self>) -> Self {
        let (owner, position, value) = finish.into_inner();
        unsafe { owner.indices.as_ptr().add(position).write(value) };
        if PLACE {
            unsafe { owner.reindex(value, position as u32) };
        }
        owner
    }
}

impl<K, V, P: Ord> Core<K, V, P> {
    fn compare(entry: hole::At<&&mut Self, &(u32, &P)>) -> cmp::Ordering {
        let (owner, position, (index, priority)) = entry.into_inner();
        // SAFETY: path planning retains this initialized heap position and its resident.
        let current = unsafe { owner.indices.as_ptr().add(position).read() };
        let other = unsafe { owner.store.pair(current) };
        priority
            .cmp(&&other.priority)
            .then_with(|| index.cmp(&current))
    }

    unsafe fn reindex(&mut self, index: u32, position: u32) {
        unsafe {
            self.store
                .edit()
                .1
                .get_unchecked_mut(index as usize)
                .assume_init_mut()
        }
        .position = position;
    }
}
