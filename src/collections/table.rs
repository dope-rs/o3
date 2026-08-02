use std::alloc::Layout;
use std::fmt;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

use crate::collections::ClearGuard;
use crate::marker::ThreadBound;

const EMPTY: u8 = u8::MAX;

enum Probe {
    Occupied(usize),
    Vacant(usize),
    Full,
}

pub struct FixedHashTablePlan<V> {
    capacity: usize,
    marker: PhantomData<fn() -> V>,
}

impl<V> Copy for FixedHashTablePlan<V> {}

impl<V> Clone for FixedHashTablePlan<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> fmt::Debug for FixedHashTablePlan<V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FixedHashTablePlan")
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl<V> FixedHashTablePlan<V> {
    pub fn new(capacity: usize) -> Option<Self> {
        if capacity > 1 << (usize::BITS - 2) {
            return None;
        }
        let buckets = (capacity * 2).next_power_of_two();
        (Layout::array::<u8>(buckets).is_ok()
            && Layout::array::<u64>(buckets).is_ok()
            && Layout::array::<V>(buckets).is_ok())
        .then_some(Self {
            capacity,
            marker: PhantomData,
        })
    }

    pub fn capacity(self) -> usize {
        self.capacity
    }
}

pub struct FixedHashTable<V> {
    controls: Box<[u8]>,
    hashes: Box<[MaybeUninit<u64>]>,
    values: Box<[MaybeUninit<V>]>,
    len: usize,
    capacity: usize,
    _thread: ThreadBound,
}

impl<V: Clone> Clone for FixedHashTable<V> {
    fn clone(&self) -> Self {
        let mut cloned = Self::with_capacity(self.capacity);
        for index in 0..self.controls.len() {
            if self.controls[index] == EMPTY {
                continue;
            }
            let hash = unsafe { self.hashes.get_unchecked(index).assume_init() };
            let value = unsafe { self.values.get_unchecked(index).assume_init_ref() }.clone();
            if cloned.try_insert(hash, value, |_| false).is_err() {
                unreachable!();
            }
        }
        cloned
    }
}

impl<V: fmt::Debug> fmt::Debug for FixedHashTable<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut entries = f.debug_list();
        for index in 0..self.controls.len() {
            if self.controls[index] != EMPTY {
                entries.entry(unsafe { self.values.get_unchecked(index).assume_init_ref() });
            }
        }
        entries.finish()
    }
}

impl<V> FixedHashTable<V> {
    pub fn capacity_fits(capacity: usize) -> bool {
        FixedHashTablePlan::<V>::new(capacity).is_some()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let plan = FixedHashTablePlan::new(capacity).expect("hash table capacity overflow");
        Self::from_plan(plan)
    }

    pub fn from_plan(plan: FixedHashTablePlan<V>) -> Self {
        let capacity = plan.capacity();
        let buckets = (capacity * 2).next_power_of_two();
        Self {
            controls: vec![EMPTY; buckets].into_boxed_slice(),
            hashes: Box::<[u64]>::new_uninit_slice(buckets),
            values: Box::<[V]>::new_uninit_slice(buckets),
            len: 0,
            capacity,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn get(&self, hash: u64, matches: impl FnMut(&V) -> bool) -> Option<&V> {
        let Probe::Occupied(index) = self.probe(hash, matches) else {
            return None;
        };
        Some(unsafe { self.values.get_unchecked(index).assume_init_ref() })
    }

    pub fn get_mut(&mut self, hash: u64, matches: impl FnMut(&V) -> bool) -> Option<&mut V> {
        let Probe::Occupied(index) = self.probe(hash, matches) else {
            return None;
        };
        Some(unsafe { self.values.get_unchecked_mut(index).assume_init_mut() })
    }

    pub fn insert(
        &mut self,
        hash: u64,
        value: V,
        matches: impl FnMut(&V) -> bool,
    ) -> Result<Option<V>, V> {
        match self.probe(hash, matches) {
            Probe::Occupied(index) => {
                use std::mem::replace;
                let current = unsafe { self.values.get_unchecked_mut(index).assume_init_mut() };
                Ok(Some(replace(current, value)))
            }
            Probe::Vacant(index) => {
                self.insert_at(index, hash, value);
                Ok(None)
            }
            Probe::Full => Err(value),
        }
    }

    pub fn try_insert(
        &mut self,
        hash: u64,
        value: V,
        matches: impl FnMut(&V) -> bool,
    ) -> Result<(), V> {
        let Probe::Vacant(index) = self.probe(hash, matches) else {
            return Err(value);
        };
        self.insert_at(index, hash, value);
        Ok(())
    }

    pub fn remove(&mut self, hash: u64, matches: impl FnMut(&V) -> bool) -> Option<V> {
        let Probe::Occupied(mut hole) = self.probe(hash, matches) else {
            return None;
        };
        self.controls[hole] = EMPTY;
        let removed = unsafe { self.values.get_unchecked(hole).assume_init_read() };
        self.len -= 1;
        let mask = self.controls.len() - 1;
        let mut next = (hole + 1) & mask;
        while self.controls[next] != EMPTY {
            let hash = unsafe { self.hashes.get_unchecked(next).assume_init() };
            let home = hash as usize & mask;
            if next.wrapping_sub(home) & mask > hole.wrapping_sub(home) & mask {
                let value = unsafe { self.values.get_unchecked(next).assume_init_read() };
                unsafe {
                    self.hashes.get_unchecked_mut(hole).write(hash);
                    self.values.get_unchecked_mut(hole).write(value);
                }
                self.controls[hole] = self.controls[next];
                self.controls[next] = EMPTY;
                hole = next;
            }
            next = (next + 1) & mask;
        }
        Some(removed)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        let controls = &self.controls;
        self.values
            .iter_mut()
            .enumerate()
            .filter_map(move |(index, value)| {
                (controls[index] != EMPTY).then(|| unsafe { value.assume_init_mut() })
            })
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.values.iter().enumerate().filter_map(|(index, value)| {
            (self.controls[index] != EMPTY).then(|| unsafe { value.assume_init_ref() })
        })
    }

    pub fn clear(&mut self) {
        for index in 0..self.controls.len() {
            if self.controls[index] == EMPTY {
                continue;
            }
            self.controls[index] = EMPTY;
            self.len -= 1;
            unsafe { self.values.get_unchecked_mut(index).assume_init_drop() };
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn insert_at(&mut self, index: usize, hash: u64, value: V) {
        debug_assert!(self.controls[index] == EMPTY && self.len < self.capacity);
        unsafe {
            self.hashes.get_unchecked_mut(index).write(hash);
            self.values.get_unchecked_mut(index).write(value);
        }
        self.controls[index] = fingerprint(hash);
        self.len += 1;
    }

    fn probe(&self, hash: u64, mut matches: impl FnMut(&V) -> bool) -> Probe {
        let mask = self.controls.len() - 1;
        let fingerprint = fingerprint(hash);
        let mut index = hash as usize & mask;
        loop {
            let control = unsafe { *self.controls.get_unchecked(index) };
            if control == EMPTY {
                return if self.len == self.capacity {
                    Probe::Full
                } else {
                    Probe::Vacant(index)
                };
            }
            if control == fingerprint
                && unsafe { self.hashes.get_unchecked(index).assume_init() } == hash
                && matches(unsafe { self.values.get_unchecked(index).assume_init_ref() })
            {
                return Probe::Occupied(index);
            }
            index = (index + 1) & mask;
        }
    }
}

impl<V> Drop for FixedHashTable<V> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}

fn fingerprint(hash: u64) -> u8 {
    (hash >> 57) as u8
}
