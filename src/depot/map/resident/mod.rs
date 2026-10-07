use std::{convert::Infallible, marker::PhantomData, mem, num::NonZero};

use crate::{
    alloc::{self, BoxSliceExt as _, VecExt as _},
    base::{
        marker::ThreadBound,
        num::{Generation, bounded},
    },
    depot::{map, queue::links},
};

mod raw;
pub mod scoped;

pub type Id = bounded::U64<0, { u64::MAX >> 2 }>;

/// The exact record and directory extents admitted before allocation.
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    records: u32,
    buckets: NonZero<u32>,
}

impl Plan {
    /// Adds admitted counts with at most 2^29 records, without rechecking them.
    /// A one-bit shift rounds the doubled count to a power of two.
    /// Zero records wrap to zero leading zeros and retain one bucket.
    pub const fn counts<const N: usize, const LIMIT: u32>(
        counts: [bounded::U32<0, LIMIT>; N],
    ) -> Self {
        const { assert!((N as u128) * (LIMIT as u128) <= 1 << 29, "capacity") };
        let mut records = 0_u32;
        let mut index = 0;
        while index < N {
            records = records.wrapping_add(counts[index].get());
            index += 1;
        }
        let shift = records.wrapping_mul(2).wrapping_sub(1).leading_zeros();
        let buckets = 1_u32.wrapping_shl(shift.wrapping_neg());
        Self {
            records,
            buckets: NonZero::<u32>::MIN.saturating_add(buckets - 1),
        }
    }
}

/// An inert coordinate; a table admits its live generation once on entry.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key<const MAX: u32>(NonZero<u64>);

impl<const MAX: u32> Key<MAX> {
    pub const fn from_raw(raw: NonZero<u64>) -> Option<Self> {
        if raw.get() as u32 != 0 && (raw.get() >> 32) as u32 <= MAX {
            Some(Self(raw))
        } else {
            None
        }
    }

    pub const fn raw(self) -> NonZero<u64> {
        self.0
    }

    pub const fn index(self) -> u32 {
        (self.0.get() as u32) - 1
    }

    pub const fn generation(self) -> u32 {
        (self.0.get() >> 32) as u32
    }

    fn new(index: links::Index, epoch: Generation<MAX>) -> Self {
        let slot = NonZero::<u64>::MIN.saturating_add(u64::from(index.raw()));
        Self(slot | (u64::from(epoch.get()) << 32))
    }

    fn slot(self) -> links::Index {
        // SAFETY: the admitted nonzero low word excludes Index::END.
        unsafe { links::Index::new_raw(self.index()) }
    }
}

/// Owns the directory, initialized record prefix, free chain and finite generations.
pub struct Table<T, H, const MAX: u32> {
    records: Vec<raw::Record<T, MAX>>,
    buckets: Box<[u32]>,
    free: Option<links::Index>,
    usable: u32,
    len: u32,
    hash: H,
    bound: PhantomData<ThreadBound>,
}

impl<T, H: map::Hasher<Id>, const MAX: u32> Table<T, H, MAX> {
    pub fn try_new(plan: Plan, hash: H) -> Result<Self, alloc::Error> {
        let records = Vec::try_new(plan.records as usize)?;
        let buckets = Box::try_repeat(plan.buckets.get() as usize, links::Index::END)?;
        Ok(Self {
            records,
            buckets,
            free: None,
            usable: plan.records,
            len: 0,
            hash,
            bound: PhantomData,
        })
    }

    pub const fn available(&self) -> usize {
        self.usable.wrapping_sub(self.len) as usize
    }

    pub fn get(&self, key: Key<MAX>) -> Option<&T> {
        let record = self.record(key)?;
        // SAFETY: record admitted this exact initialized generation.
        Some(unsafe { record.value.assume_init_ref() })
    }

    pub fn lookup(&self, key: Key<MAX>, id: Id) -> Option<&T> {
        let record = self.record(key)?;
        if record.id != id.get() {
            return None;
        }
        Some(unsafe { record.value.assume_init_ref() })
    }

    pub fn id(&self, key: Key<MAX>) -> Option<Id> {
        Some(self.record(key)?.id())
    }

    pub fn find(&self, id: Id) -> Option<(Key<MAX>, &T)> {
        let map::raw::State::Occupied(bucket) = self.scan(id) else {
            return None;
        };
        // SAFETY: the retained occupied bucket names a live record in this owner.
        let index = unsafe { self.index(&bucket) };
        let record = unsafe { self.at(index) };
        Some((Key::new(index, record.generation), unsafe {
            record.value.assume_init_ref()
        }))
    }

    pub fn key(&self, id: Id) -> Option<Key<MAX>> {
        self.find(id).map(|(key, _)| key)
    }

    pub fn entry(
        &mut self,
        id: Id,
    ) -> map::Entry<Occupied<'_, T, H, MAX>, Option<Vacant<'_, T, H, MAX>>> {
        match self.scan(id) {
            map::raw::State::Occupied(bucket) => {
                let index = unsafe { self.index(&bucket) };
                map::Entry::Occupied(Occupied {
                    table: self,
                    bucket: Some(bucket),
                    index,
                })
            }
            map::raw::State::Vacant(bucket) if self.len < self.usable => {
                map::Entry::Vacant(Some(Vacant {
                    table: self,
                    bucket,
                    id,
                }))
            }
            map::raw::State::Vacant(_) | map::raw::State::Full => map::Entry::Vacant(None),
        }
    }

    /// Admits identity and generation once, retaining the owner even on rejection.
    pub fn exact(&mut self, key: Key<MAX>, id: Id) -> Result<Occupied<'_, T, H, MAX>, &mut Self> {
        if !self.record(key).is_some_and(|record| record.id == id.get()) {
            return Err(self);
        }
        Ok(Occupied {
            table: self,
            bucket: None,
            index: key.slot(),
        })
    }

    pub fn scope<R>(
        &mut self,
        run: impl for<'id> FnOnce(scoped::Scope<'_, 'id, T, H, MAX>) -> R,
    ) -> R {
        run(scoped::Scope::new(self))
    }

    fn record(&self, key: Key<MAX>) -> Option<&raw::Record<T, MAX>> {
        let record = self.records.get(key.index() as usize)?;
        record.matches(key).then_some(record)
    }

    fn scan(&self, id: Id) -> map::raw::State {
        match map::raw::scan::<_, Infallible, true>(self, self.hash.hash(&id), |bucket| {
            let index = unsafe { self.index(&bucket) };
            Ok(unsafe { self.at(index) }.id == id.get())
        }) {
            Ok(state) => state,
            Err(error) => match error {},
        }
    }

    /// # Safety
    /// The retained bucket is occupied in this exact directory.
    unsafe fn index(&self, bucket: &map::raw::Occupied) -> links::Index {
        let raw = unsafe { *self.buckets.get_unchecked(bucket.index()) };
        unsafe { links::Index::new_raw(raw) }
    }

    /// # Safety
    /// This index belongs to the initialized prefix of this exact owner.
    unsafe fn at(&self, index: links::Index) -> &raw::Record<T, MAX> {
        unsafe { self.records.get_unchecked(index.get()) }
    }

    /// # Safety
    /// The caller exclusively retains this live, indexed record.
    unsafe fn member(&self, index: links::Index) -> map::raw::Occupied {
        let id = unsafe { self.at(index) }.id();
        unsafe {
            map::raw::member(self, self.hash.hash(&id), |bucket| {
                self.index(&bucket) == index
            })
        }
    }
}

impl<T, H, const MAX: u32> Drop for Table<T, H, MAX> {
    fn drop(&mut self) {
        if !mem::needs_drop::<T>() {
            return;
        }
        for record in &mut self.records {
            if record.is_live() {
                // SAFETY: each live record owns one initialized value.
                unsafe { record.value.assume_init_drop() };
            }
        }
    }
}

// SAFETY: only this owner publishes bucket indices, after initializing their
// exact record; removal closes the directory before recycling that record.
unsafe impl<T, H: map::Hasher<Id>, const MAX: u32> map::raw::Directory for Table<T, H, MAX> {
    fn buckets(&self) -> usize {
        self.buckets.len()
    }

    unsafe fn hash(&self, index: usize) -> Option<u64> {
        let raw = unsafe { *self.buckets.get_unchecked(index) };
        let index = links::Index::decode(raw)?;
        Some(self.hash.hash(&unsafe { self.at(index) }.id()))
    }

    unsafe fn candidate(&self, index: usize, _: u64) -> map::raw::Candidate {
        if unsafe { *self.buckets.get_unchecked(index) } == links::Index::END {
            map::raw::Candidate::Vacant
        } else {
            map::raw::Candidate::Occupied
        }
    }
}

// SAFETY: the scalar directory stores the exact live index in one word;
// relocation leaves its record untouched and vacates the source bucket.
unsafe impl<T, H: map::Hasher<Id>, const MAX: u32> map::raw::Shift for Table<T, H, MAX> {
    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, _: u64) {
        let raw = unsafe { *self.buckets.get_unchecked(next.index()) };
        unsafe {
            *self.buckets.get_unchecked_mut(hole.index()) = raw;
            *self.buckets.get_unchecked_mut(next.index()) = links::Index::END;
        }
    }
}

/// An occupied bucket retaining both its directory and live record.
pub struct Occupied<'a, T, H, const MAX: u32> {
    table: &'a mut Table<T, H, MAX>,
    bucket: Option<map::raw::Occupied>,
    index: links::Index,
}

impl<'a, T, H: map::Hasher<Id>, const MAX: u32> Occupied<'a, T, H, MAX> {
    pub fn key(&self) -> Key<MAX> {
        Key::new(self.index, unsafe { self.table.at(self.index) }.generation)
    }

    pub fn id(&self) -> Id {
        unsafe { self.table.at(self.index) }.id()
    }

    pub fn get(&self) -> &T {
        unsafe { self.table.at(self.index).value.assume_init_ref() }
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe {
            self.table
                .records
                .get_unchecked_mut(self.index.get())
                .value
                .assume_init_mut()
        }
    }

    pub fn remove(self) -> T {
        let bucket = match self.bucket {
            Some(bucket) => bucket,
            None => unsafe { self.table.member(self.index) },
        };
        unsafe { raw::remove(self.table, bucket, self.index) }
    }
}

/// A vacant bucket and one record credit in the same exclusive owner.
pub struct Vacant<'a, T, H, const MAX: u32> {
    table: &'a mut Table<T, H, MAX>,
    bucket: map::raw::Vacant,
    id: Id,
}

impl<'a, T, H: map::Hasher<Id>, const MAX: u32> Vacant<'a, T, H, MAX> {
    pub fn insert(self, value: T) -> Occupied<'a, T, H, MAX> {
        let Self { table, bucket, id } = self;
        let offset = bucket.index();
        let index = unsafe { raw::insert(table, bucket, id, value) };
        // SAFETY: insertion initialized and published this exact bucket.
        let bucket = unsafe { map::raw::Occupied::new(offset) };
        Occupied {
            table,
            bucket: Some(bucket),
            index,
        }
    }
}

const _: () = {
    assert!(mem::size_of::<raw::Record<[u32; 3], { u32::MAX }>>() == 24);
    assert!(mem::size_of::<Key<{ u32::MAX }>>() == 8);
    assert!(mem::size_of::<Option<Key<{ u32::MAX }>>>() == 8);
};
