use std::{marker, mem};

use priority::errors;

use crate::{
    base::marker::ThreadBound,
    depot::{
        index::Initialized,
        map::{self, priority},
        queue::links,
    },
};

/// A fixed-capacity map whose entries are ordered by minimum priority.
pub struct Min<K, P: Ord, V, H> {
    pub(in crate::depot::map::priority) core: priority::Core<K, V, P>,
    thread: marker::PhantomData<ThreadBound>,
    pub(in crate::depot::map::priority) hasher: H,
}

impl<K, P: Ord, V, H> Min<K, P, V, H> {
    pub fn try_new(
        plan: priority::Plan<K, P, V>,
        hasher: H,
    ) -> Result<Self, crate::alloc::Exhausted> {
        Ok(Self {
            core: priority::Core::try_new(plan)?,
            thread: marker::PhantomData,
            hasher,
        })
    }

    pub fn peek(&self) -> Option<(&K, &P, &V)> {
        if self.is_empty() {
            return None;
        }
        let index = unsafe { self.core.indices.as_ptr().read() };
        let pair = unsafe { self.core.store.pair(index) };
        Some((&pair.key, &pair.priority, &pair.value))
    }

    /// Retains the current minimum's exact live slot and exclusive source borrow.
    pub fn peek_mut(&mut self) -> Option<priority::Occupied<'_, K, P, V>> {
        if self.is_empty() {
            return None;
        }
        let index = unsafe { self.core.indices.as_ptr().read() };
        let bucket = unsafe { map::raw::Occupied::new(self.core.store.bucket(index)) };
        Some(priority::Occupied {
            core: &mut self.core,
            bucket,
            index,
        })
    }

    pub fn len(&self) -> usize {
        self.core.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K, P: Ord, V, H> Min<K, P, V, H> {
    pub(in crate::depot) fn entry_at(
        source: crate::depot::raw::Proof<(&mut Self, links::Index), Initialized<()>>,
    ) -> priority::Occupied<'_, K, P, V> {
        let (owner, index) = source.into_inner();
        let index = index.raw();
        // SAFETY: the retained live-node proof grants access to its exact current bucket.
        let bucket = unsafe { map::raw::Occupied::new(owner.core.store.bucket(index)) };
        priority::Occupied {
            core: &mut owner.core,
            bucket,
            index,
        }
    }
}

const _: () = {
    use priority::Core;

    assert!(mem::size_of::<Min<(), (), (), ()>>() == mem::size_of::<Core<(), (), ()>>());
    assert!(mem::align_of::<Min<(), (), (), ()>>() == mem::align_of::<Core<(), (), ()>>());
    assert!(
        mem::size_of::<Option<Min<(), (), (), ()>>>() == mem::size_of::<Option<Core<(), (), ()>>>()
    );
};

impl<K, P, V, H> Min<K, P, V, H>
where
    K: Eq,
    P: Ord,
    H: map::Hasher<K>,
{
    /// Retains the query key for insertion, returning it unchanged when full.
    pub fn try_entry<'a>(
        &'a mut self,
        key: K,
    ) -> Result<priority::Entry<'a, K, P, V>, crate::mem::Rejected<K, errors::Full>> {
        let hash = self.hasher.hash(&key);
        match map::raw::Probe::new(&mut self.core, hash, |pair| pair.key.eq(&key)).entry() {
            map::Entry::Occupied(raw) => Ok(priority::Entry::Occupied(raw)),
            map::Entry::Vacant(Some(entry)) => {
                Ok(priority::Entry::Vacant(priority::Vacant { entry, key }))
            }
            map::Entry::Vacant(None) => Err(crate::mem::Rejected::new(key, errors::Full)),
        }
    }

    /// Returns an occupied entry when the key is present.
    pub fn occupied<'a>(&'a mut self, key: &K) -> Option<priority::Occupied<'a, K, P, V>> {
        let hash = self.hasher.hash(key);
        map::raw::Probe::find(&mut self.core, hash, |pair| pair.key.eq(key))
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        map::raw::Probe::find(&self.core, self.hasher.hash(key), |pair| pair.key.eq(key))
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        Some(self.occupied(key)?.remove())
    }

    pub fn pop(&mut self) -> Option<(K, P, V)> {
        let occupied = self.peek_mut()?;
        let (key, value, priority) = occupied.remove_parts();
        Some((key, priority, value))
    }

    pub fn pop_if(&mut self, test: impl FnOnce(&P) -> bool) -> Option<(K, P, V)> {
        let occupied = self.peek_mut()?;
        if !test(occupied.priority()) {
            return None;
        }
        let (key, value, priority) = occupied.remove_parts();
        Some((key, priority, value))
    }
}
