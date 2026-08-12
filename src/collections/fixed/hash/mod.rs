use std::{fmt, marker, mem};

pub mod raw;

const EMPTY: u8 = u8::MAX;

enum Probe {
    Occupied(usize),
    Vacant(usize),
    Full,
}

pub struct Plan<V> {
    capacity: usize,
    marker: marker::PhantomData<fn() -> V>,
}

impl<V> Copy for Plan<V> {}

impl<V> Clone for Plan<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> fmt::Debug for Plan<V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Plan")
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl<V> Plan<V> {
    pub const fn new(capacity: usize) -> Option<Self> {
        if Self::fits(capacity) {
            Some(Self {
                capacity,
                marker: marker::PhantomData,
            })
        } else {
            None
        }
    }

    pub const fn fixed<const N: usize>() -> Self {
        assert!(
            Self::fits(N),
            "hash table capacity exceeds allocation layout"
        );
        Self {
            capacity: N,
            marker: marker::PhantomData,
        }
    }

    pub const fn capacity(self) -> usize {
        self.capacity
    }

    const fn fits(capacity: usize) -> bool {
        use std::alloc::Layout;

        if capacity > 1 << (usize::BITS - 2) {
            return false;
        }
        let buckets = (capacity * 2).next_power_of_two();
        Layout::array::<u8>(buckets).is_ok()
            && Layout::array::<u64>(buckets).is_ok()
            && Layout::array::<V>(buckets).is_ok()
    }
}

pub struct Map<V> {
    controls: Box<[u8]>,
    hashes: Box<[mem::MaybeUninit<u64>]>,
    values: Box<[mem::MaybeUninit<V>]>,
    len: usize,
    capacity: usize,
    _thread: crate::ThreadBound,
}

pub enum Entry<'a, V> {
    Occupied(Occupied<'a, V>),
    Vacant(Vacant<'a, V>),
}

pub struct Occupied<'a, V> {
    map: &'a mut Map<V>,
    index: usize,
}

pub struct Vacant<'a, V> {
    map: &'a mut Map<V>,
    index: usize,
    hash: u64,
}

impl<V> Occupied<'_, V> {
    pub fn get(&self) -> &V {
        unsafe { self.map.values.get_unchecked(self.index).assume_init_ref() }
    }

    pub fn get_mut(&mut self) -> &mut V {
        unsafe {
            self.map
                .values
                .get_unchecked_mut(self.index)
                .assume_init_mut()
        }
    }

    pub fn remove(self) -> V {
        self.map.remove_at(self.index)
    }
}

impl<'a, V> Vacant<'a, V> {
    pub fn insert(self, value: V) -> &'a mut V {
        self.map.insert_at(self.index, self.hash, value);
        unsafe {
            self.map
                .values
                .get_unchecked_mut(self.index)
                .assume_init_mut()
        }
    }
}

impl<V: Clone> Clone for Map<V> {
    fn clone(&self) -> Self {
        let mut cloned = Self::from_plan(Plan {
            capacity: self.capacity,
            marker: marker::PhantomData,
        });
        for index in 0..self.controls.len() {
            if self.controls[index] == EMPTY {
                continue;
            }
            let hash = unsafe { self.hashes.get_unchecked(index).assume_init() };
            let value = unsafe { self.values.get_unchecked(index).assume_init_ref() }.clone();
            cloned.insert_at(index, hash, value);
        }
        cloned
    }
}

impl<V: fmt::Debug> fmt::Debug for Map<V> {
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

impl<V> Map<V> {
    pub fn from_plan(plan: Plan<V>) -> Self {
        match Self::try_from_plan(plan) {
            Ok(map) => map,
            Err(error) => error.abort(),
        }
    }

    pub fn try_from_plan(plan: Plan<V>) -> Result<Self, crate::collections::AllocationError> {
        use crate::ThreadBound;
        let capacity = plan.capacity();
        let buckets = (capacity * 2).next_power_of_two();
        Ok(Self {
            controls: crate::collections::try_box_with(buckets, |_| EMPTY)?,
            hashes: crate::collections::try_box_uninit(buckets)?,
            values: crate::collections::try_box_uninit(buckets)?,
            len: 0,
            capacity,
            _thread: ThreadBound::NEW,
        })
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

    pub fn entry(&mut self, hash: u64, matches: impl FnMut(&V) -> bool) -> Option<Entry<'_, V>> {
        match self.probe(hash, matches) {
            Probe::Occupied(index) => Some(Entry::Occupied(Occupied { map: self, index })),
            Probe::Vacant(index) => Some(Entry::Vacant(Vacant {
                map: self,
                index,
                hash,
            })),
            Probe::Full => None,
        }
    }

    pub fn try_insert(
        &mut self,
        hash: u64,
        value: V,
        matches: impl FnMut(&V) -> bool,
    ) -> Result<(), V> {
        let Some(Entry::Vacant(entry)) = self.entry(hash, matches) else {
            return Err(value);
        };
        entry.insert(value);
        Ok(())
    }

    pub fn remove(&mut self, hash: u64, matches: impl FnMut(&V) -> bool) -> Option<V> {
        let Probe::Occupied(index) = self.probe(hash, matches) else {
            return None;
        };
        Some(self.remove_at(index))
    }

    fn remove_at(&mut self, mut hole: usize) -> V {
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
        removed
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
        self.values
            .iter()
            .enumerate()
            .filter(|(index, _)| self.controls[*index] != EMPTY)
            .map(|(_, value)| {
                // SAFETY: non-empty control bytes identify initialized slots.
                unsafe { value.assume_init_ref() }
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

impl<V> Drop for Map<V> {
    fn drop(&mut self) {
        use crate::collections::ClearGuard;
        ClearGuard::run(self, Self::clear);
    }
}

fn fingerprint(hash: u64) -> u8 {
    (hash >> 57) as u8
}
