use std::{marker, mem};

pub use entry::Entry;

use crate::{alloc, base::marker::ThreadBound, depot::map};

mod entry;
pub mod errors;

pub struct Table<V> {
    pub(in crate::depot::map::flat) table: map::raw::Table<V>,
    pub(in crate::depot::map::flat) len: u32,
    pub(in crate::depot::map::flat) shape: map::raw::Shape<V>,
    thread: marker::PhantomData<ThreadBound>,
}

impl<V> Table<V> {
    /// Consumes one retained logical admission without comparing existing values.
    /// The original context is returned after its capacity has been used.
    pub fn insert<C>(
        source: crate::depot::raw::Proof<(&mut Self, C), raw::Available>,
        hash: u64,
        value: V,
    ) -> (&mut V, C) {
        let (table, context) = source.into_inner();
        // SAFETY: the consumed admission retains logical and directory capacity.
        let entry = unsafe { map::raw::Probe::vacant(table, hash) };
        (entry.insert(value), context)
    }

    pub fn admit<C>(
        source: crate::depot::raw::Proof<(&mut Self, C), raw::Available>,
        hash: u64,
        matches: impl Fn(&V) -> bool,
    ) -> (Entry<'_, V>, C) {
        let (table, context) = source.into_inner();
        // SAFETY: the consumed admission retains logical and directory capacity.
        let entry = match unsafe { map::raw::Probe::spare(table, hash, matches) } {
            map::Entry::Occupied(entry) => Entry::Occupied(entry),
            map::Entry::Vacant(entry) => Entry::Vacant(entry),
        };
        (entry, context)
    }

    pub fn member<C, F: Fn(&V) -> bool>(
        source: crate::depot::raw::Proof<(&mut Self, C, u64, F), raw::Member>,
    ) -> (map::Found<&mut Self>, C) {
        let (table, context, hash, matches) = source.into_inner();
        // SAFETY: the same consumed proof binds this directory, hash, matcher and live context.
        (
            unsafe { map::raw::Probe::member(table, hash, matches) },
            context,
        )
    }

    pub fn try_new(plan: map::Plan) -> Result<Self, alloc::Error> {
        let layout = map::raw::Layout::try_plan(plan)?;
        let shape = layout.shape();
        let table = map::raw::Table::try_new(layout)?;
        Ok(Self {
            table,
            len: 0,
            shape,
            thread: marker::PhantomData,
        })
    }

    pub fn get(&self, hash: u64, matches: impl Fn(&V) -> bool) -> Option<&V> {
        map::raw::Probe::find(self, hash, matches)
    }

    pub fn get_mut(&mut self, hash: u64, matches: impl Fn(&V) -> bool) -> Option<&mut V> {
        Some(self.occupied(hash, matches)?.into_mut())
    }

    pub fn entry(&mut self, hash: u64, matches: impl Fn(&V) -> bool) -> Option<Entry<'_, V>> {
        match map::raw::Probe::new(self, hash, matches).entry() {
            map::Entry::Occupied(occupied) => Some(Entry::Occupied(occupied)),
            map::Entry::Vacant(vacant) => vacant.map(Entry::Vacant),
        }
    }

    pub fn occupied(
        &mut self,
        hash: u64,
        matches: impl Fn(&V) -> bool,
    ) -> Option<map::Found<&mut Table<V>>> {
        map::raw::Probe::find(self, hash, matches)
    }

    pub fn try_insert(
        &mut self,
        hash: u64,
        value: V,
        matches: impl Fn(&V) -> bool,
    ) -> Result<(), errors::Insert<V>> {
        match map::raw::Probe::reserve(self, hash, matches) {
            map::Entry::Occupied(()) => Err(errors::Insert::Occupied(value)),
            map::Entry::Vacant(None) => Err(errors::Insert::Full(value)),
            map::Entry::Vacant(Some(vacant)) => {
                vacant.insert(value);
                Ok(())
            }
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub fn remove(&mut self, hash: u64, matches: impl Fn(&V) -> bool) -> Option<V> {
        Some(self.occupied(hash, matches)?.remove())
    }

    pub fn retire(&mut self, hash: u64, matches: impl Fn(&V) -> bool) {
        if let Some(occupied) = self.occupied(hash, matches) {
            occupied.discard();
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.table.values()
    }

    pub fn clear(&mut self) {
        if self.is_empty() {
            return;
        }
        self.table.clear();
        self.len = 0;
    }
}

// SAFETY: logical capacity is at most half the nonempty directory's bucket count.
unsafe impl<'a, V> map::raw::Query for &'a Table<V> {
    const SPARE: bool = true;

    type Payload = V;
    type Value = V;
    type Occupied = &'a V;

    fn table(&self) -> &map::raw::Table<V> {
        &self.table
    }

    fn value(entry: map::Found<&Self>) -> &V {
        entry.payload()
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, bucket) = entry.into_parts();
        unsafe { owner.table.payload(&bucket) }
    }
}

// SAFETY: vacancy admission bounds live payloads by the same logical capacity.
unsafe impl<'a, V> map::raw::Query for &'a mut Table<V> {
    const SPARE: bool = true;

    type Payload = V;
    type Value = V;
    type Occupied = map::Found<&'a mut Table<V>>;

    fn table(&self) -> &map::raw::Table<V> {
        &self.table
    }

    fn value(entry: map::Found<&Self>) -> &V {
        entry.payload()
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        entry
    }
}

unsafe impl<'a, V> map::raw::Insertion for &'a mut Table<V> {
    type Vacant = map::Vacancy<&'a mut Table<V>, u64>;

    fn vacant(entry: map::Vacancy<Self>, hash: u64) -> Option<Self::Vacant> {
        Self::Vacant::new(entry, hash)
    }
}

impl<V> Drop for Table<V> {
    fn drop(&mut self) {
        let layout = self.shape.layout();
        let pointer = self.table.base();
        if mem::needs_drop::<V>() {
            self.clear();
        }
        unsafe { layout.bundle().deallocate(pointer) };
    }
}

const _: () = {
    assert!(mem::size_of::<Table<()>>() == 5 * mem::size_of::<usize>());
    assert!(mem::align_of::<Table<()>>() == mem::align_of::<usize>());
    assert!(mem::size_of::<Option<Table<()>>>() == mem::size_of::<Table<()>>());
    assert!(mem::size_of::<map::raw::Probe<&mut Table<u64>>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<map::Found<&mut Table<u64>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<map::Vacancy<&mut Table<u64>, u64>>() == 3 * mem::size_of::<usize>());
};

impl<'a, V> map::Found<&'a mut Table<V>> {
    pub fn into_mut(self) -> &'a mut V {
        value(self)
    }
}

impl<V> map::Found<&mut Table<V>> {
    pub fn get(&self) -> &V {
        self.read().payload()
    }

    pub fn get_mut(&mut self) -> &mut V {
        value(self.reborrow())
    }

    pub fn remove(self) -> V {
        let (map, bucket) = self.into_parts();
        map.len -= 1;
        // SAFETY: the consumed entry retains this exact occupied bucket and exclusive owner.
        unsafe { map::raw::Hole::remove(map, bucket) }
    }

    pub fn discard(self) {
        drop(self.remove());
    }
}

impl<'a, V> map::Vacancy<&'a mut Table<V>, u64> {
    pub(in crate::depot::map::flat) fn new(
        entry: map::Vacancy<&'a mut Table<V>>,
        hash: u64,
    ) -> Option<Self> {
        let map = entry.source();
        if map.len as usize >= map.shape.capacity().get() {
            return None;
        }
        Some(entry.with_phase(hash))
    }

    pub fn insert(self, value: V) -> &'a mut V {
        let (map, bucket, hash) = self.into_parts();
        // SAFETY: this capability owns the same vacant bucket through insertion.
        let value = unsafe { map.table.insert(bucket, hash, value) };
        map.len += 1;
        value
    }
}

fn value<V>(entry: map::Found<&mut Table<V>>) -> &mut V {
    let (map, bucket) = entry.into_parts();
    // SAFETY: the consumed capability retains this initialized payload's original exclusive loan.
    unsafe { map.table.payload_mut(&bucket) }
}

pub mod raw;
