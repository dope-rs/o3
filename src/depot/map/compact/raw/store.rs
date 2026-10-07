use std::{marker, num};

use crate::{depot, depot::map::compact};

pub(in crate::depot::map::compact) struct Store<'id, Q: compact::raw::Read> {
    owner: Q,
    layout: compact::Layout,
    buckets: num::NonZero<u32>,
    brand: marker::PhantomData<fn(&'id ()) -> &'id ()>,
}

impl<Q: compact::raw::Read> Store<'_, Q> {
    pub(in crate::depot::map::compact) fn with_layout<R>(
        owner: Q,
        acquire: impl for<'id> FnOnce(Store<'id, Q>) -> R,
        empty: impl FnOnce(Q) -> R,
    ) -> R {
        let layout = owner.layout();
        let Some(buckets) = num::NonZero::new(layout.buckets) else {
            return empty(owner);
        };
        acquire(Store {
            owner,
            layout,
            buckets,
            brand: marker::PhantomData,
        })
    }

    /// # Safety
    /// The retained owner has a nonempty directory.
    pub(in crate::depot::map::compact) unsafe fn with<R>(
        owner: Q,
        f: impl for<'id> FnOnce(Store<'id, Q>) -> R,
    ) -> R {
        let layout = owner.layout();
        // SAFETY: the retained admission proves this owner's bucket domain nonzero.
        let buckets = unsafe { num::NonZero::new_unchecked(layout.buckets) };
        f(Store {
            owner,
            layout,
            buckets,
            brand: marker::PhantomData,
        })
    }
}

impl<'id, Q: compact::raw::Read> Store<'id, Q> {
    fn bucket(&self, index: usize) -> Bucket<'id> {
        Bucket {
            index: index & (self.buckets.get() as usize - 1),
            brand: self.brand,
        }
    }

    fn control(&self, bucket: Bucket<'id>) -> Control<'id> {
        let value = unsafe {
            *self
                .owner
                .bytes()
                .as_ptr()
                .add(self.layout.start as usize + bucket.index)
        };
        Control { bucket, value }
    }

    fn read(&self, resident: Resident<'id>) -> Entry<'id> {
        let bucket = resident.bucket;
        let width = compact::Width::new(self.layout.domain);
        let start = self.layout.plane() + bucket.index * width as usize;
        let bytes = unsafe { self.owner.bytes().as_ptr().add(start) };
        let index = unsafe {
            match width {
                compact::Width::One => u32::from(bytes.read()),
                compact::Width::Two => {
                    u32::from(u16::from_le(bytes.cast::<u16>().read_unaligned()))
                }
                compact::Width::Four => u32::from_le(bytes.cast::<u32>().read_unaligned()),
            }
        };
        Entry {
            bucket,
            payload: Payload {
                index,
                brand: self.brand,
            },
        }
    }

    fn value(&self, payload: Payload<'id>) -> compact::Value<'_, Q> {
        compact::Value::new(unsafe { depot::raw::Proof::new((&self.owner, payload.index)) })
    }
}

impl<'id, Q: compact::raw::Write> Store<'id, Q> {
    fn write(&mut self, bucket: Bucket<'id>, control: u8, payload: Payload<'id>) {
        self.mark(bucket, control);
        let width = compact::Width::new(self.layout.domain);
        let start = self.layout.plane() + bucket.index * width as usize;
        let bytes = unsafe { self.owner.bytes_mut().as_mut_ptr().add(start) };
        unsafe {
            match width {
                compact::Width::One => bytes.write(payload.index as u8),
                compact::Width::Two => bytes
                    .cast::<u16>()
                    .write_unaligned((payload.index as u16).to_le()),
                compact::Width::Four => bytes.cast::<u32>().write_unaligned(payload.index.to_le()),
            }
        }
    }

    fn mark(&mut self, bucket: Bucket<'id>, value: u8) {
        let index = self.layout.start as usize + bucket.index;
        let bytes = unsafe { self.owner.bytes_mut().as_mut_ptr() };
        unsafe { bytes.add(index).write(value) };
        if bucket.index < 32 {
            unsafe { bytes.add(index + self.buckets.get() as usize).write(value) };
        }
    }
}

impl<Q: compact::raw::Insert> Store<'_, Q> {
    pub(in crate::depot::map::compact) fn insert(mut self) -> Q::Output {
        let mut payload = Payload {
            index: self.owner.input().index,
            brand: self.brand,
        };
        let hash = Q::hash(self.value(payload));
        let mut bucket = self.bucket(hash as usize);
        let mut home = bucket;
        let mut control = (hash >> 57) as u8;
        loop {
            let Some(resident) = self.control(bucket).occupied() else {
                self.write(bucket, control, payload);
                return unsafe { self.owner.inserted() };
            };
            let current = resident.control;
            let stored = self.read(resident).payload;
            let origin = self.bucket(Q::hash(self.value(stored)) as usize);
            if (bucket.index.wrapping_sub(home.index) & (self.buckets.get() as usize - 1))
                > (bucket.index.wrapping_sub(origin.index) & (self.buckets.get() as usize - 1))
            {
                self.write(bucket, control, payload);
                control = current;
                payload = stored;
                home = origin;
            }
            bucket = self.bucket(bucket.index + 1);
        }
    }
}

impl<Q: compact::raw::Member> Store<'_, Q> {
    pub(in crate::depot::map::compact) fn locate(self) -> compact::Found<Q> {
        let payload = Payload {
            index: self.owner.input().index,
            brand: self.brand,
        };
        let hash = Q::hash(self.value(payload));
        let mut bucket = self.bucket(hash as usize);
        loop {
            if let Some(resident) = self.control(bucket).matching(hash) {
                let entry = self.read(resident);
                if entry.payload == payload {
                    return entry.finish(self);
                }
            }
            bucket = self.bucket(bucket.index + 1);
        }
    }
}

impl<Q: compact::raw::Remove> Store<'_, Q> {
    pub(in crate::depot::map::compact) fn remove(occupied: compact::Found<Q>) -> Q::Output {
        let compact::Found {
            owner,
            bucket,
            payload,
        } = occupied;
        // SAFETY: the occupied member retains its nonempty directory.
        unsafe {
            Self::with(owner, |store| {
                let hole = store.bucket(bucket.get() as usize - 1);
                let payload = Payload {
                    index: payload,
                    brand: store.brand,
                };
                store.erase(hole, payload)
            })
        }
    }
}

impl<'id, Q: compact::raw::Remove> Store<'id, Q> {
    fn erase(mut self, mut hole: Bucket<'id>, payload: Payload<'id>) -> Q::Output {
        let mut next = self.bucket(hole.index + 1);
        while let Some(resident) = self.control(next).occupied() {
            let control = resident.control;
            let payload = self.read(resident).payload;
            let home = self.bucket(Q::hash(self.value(payload)) as usize);
            if next.index == home.index {
                break;
            }
            self.write(hole, control, payload);
            hole = next;
            next = self.bucket(next.index + 1);
        }
        self.mark(hole, compact::EMPTY);
        Q::removed(compact::Removed {
            owner: self.owner,
            payload: payload.index,
        })
    }
}

#[derive(Clone, Copy)]
struct Bucket<'id> {
    index: usize,
    brand: marker::PhantomData<fn(&'id ()) -> &'id ()>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Payload<'id> {
    index: u32,
    brand: marker::PhantomData<fn(&'id ()) -> &'id ()>,
}

struct Resident<'id> {
    bucket: Bucket<'id>,
    control: u8,
}

struct Control<'id> {
    bucket: Bucket<'id>,
    value: u8,
}

impl<'id> Control<'id> {
    fn occupied(self) -> Option<Resident<'id>> {
        if self.value == compact::EMPTY {
            return None;
        }
        Some(Resident {
            bucket: self.bucket,
            control: self.value,
        })
    }

    fn matching(self, hash: u64) -> Option<Resident<'id>> {
        if self.value != (hash >> 57) as u8 {
            return None;
        }
        Some(Resident {
            bucket: self.bucket,
            control: self.value,
        })
    }
}

struct Entry<'id> {
    bucket: Bucket<'id>,
    payload: Payload<'id>,
}

impl<'id> Entry<'id> {
    fn finish<Q: compact::raw::Read>(self, store: Store<'id, Q>) -> compact::Found<Q> {
        // SAFETY: Entry retains this bounded occupied bucket and its exact payload.
        let bucket = unsafe { num::NonZero::new_unchecked(self.bucket.index as u32 + 1) };
        compact::Found {
            owner: store.owner,
            bucket,
            payload: self.payload.index,
        }
    }
}

struct Matches<'a, 'id, Q: compact::raw::Read> {
    store: &'a Store<'id, Q>,
    hash: u64,
    start: Bucket<'id>,
    bits: u32,
    done: bool,
}

impl<'a, 'id, Q: compact::raw::Read> Matches<'a, 'id, Q> {
    fn new(store: &'a Store<'id, Q>, bucket: Bucket<'id>, hash: u64) -> Self {
        let start = store.layout.start as usize + bucket.index;
        // SAFETY: B admitted controls plus 32 initialized mirror bytes retain
        // this complete 32-byte group starting at any bounded bucket below B.
        let bytes = unsafe { &*store.owner.bytes().as_ptr().add(start).cast::<[u8; 32]>() };
        let (vacant, matched) = super::simd::group_masks(bytes, (hash >> 57) as u8);
        let before = vacant.wrapping_sub(1) & !vacant;
        Self {
            store,
            hash,
            start: bucket,
            bits: matched & before,
            done: vacant != 0,
        }
    }
}

impl<'id, Q: compact::raw::Source> Iterator for Matches<'_, 'id, Q> {
    type Item = Entry<'id>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let Some(bits) = num::NonZero::new(self.bits) else {
                if self.done {
                    return None;
                }
                *self = Self::new(
                    self.store,
                    self.store.bucket(self.start.index + 32),
                    self.hash,
                );
                continue;
            };
            self.bits = bits.get() & (bits.get() - 1);
            let bucket = self
                .store
                .bucket(self.start.index + bits.trailing_zeros() as usize);
            // SAFETY: the matching control retains this initialized payload. A
            // power-of-two bucket count >=32 and 1/2/4-byte widths make the plane a
            // multiple of four, containing this payload's complete aligned-down word.
            let width = compact::Width::new(self.store.layout.domain) as usize;
            let offset = bucket.index * width;
            let start = self.store.layout.plane() + (offset & !3);
            let bytes = unsafe { self.store.owner.bytes().as_ptr().add(start) };
            let word = u32::from_le(unsafe { bytes.cast::<u32>().read_unaligned() });
            let mask = u32::MAX >> (32 - width * 8);
            let entry = Entry {
                bucket,
                payload: Payload {
                    index: (word >> ((offset & 3) * 8)) & mask,
                    brand: self.start.brand,
                },
            };
            if Q::hash(self.store.value(entry.payload)) == self.hash {
                return Some(entry);
            }
        }
    }
}

const _: () = assert!(size_of::<Payload<'_>>() == size_of::<u32>());

impl<'id, Q: compact::raw::Query> Store<'id, Q> {
    pub(in crate::depot::map::compact) fn find(
        self,
        hash: u64,
        key: &Q::Item,
    ) -> Option<compact::Found<Q>>
    where
        Q: compact::raw::Remove,
        Q::Item: Eq,
    {
        let store = self;
        let tag = (hash >> 57) as u8;
        let mut bucket = store.bucket(hash as usize);
        loop {
            let resident = store.control(bucket).occupied()?;
            if resident.control == tag {
                let entry = store.read(resident);
                let payload = entry.payload;
                if Q::hash(store.value(payload)) == hash && Q::get(store.value(payload)) == *key {
                    return Some(entry.finish(store));
                }
            }
            bucket = store.bucket(bucket.index + 1);
        }
    }

    /// Visits matching items while retaining the exact directory owner.
    pub(in crate::depot::map::compact) fn scan(
        self,
        hash: u64,
        mut visit: impl FnMut(Q::Item),
    ) -> Q {
        for entry in Matches::new(&self, self.bucket(hash as usize), hash) {
            visit(Q::get(self.value(entry.payload)));
        }
        self.owner
    }

    pub(in crate::depot::map::compact) fn entry(self, hash: u64, key: &Q::Item) -> compact::Entry<Q>
    where
        Q::Item: Eq,
    {
        let entry = Matches::new(&self, self.bucket(hash as usize), hash)
            .find(|entry| Q::get(self.value(entry.payload)) == *key);
        match entry {
            Some(entry) => compact::Entry::Occupied(entry.finish(self)),
            None => compact::Entry::Vacant(self.owner),
        }
    }
}
