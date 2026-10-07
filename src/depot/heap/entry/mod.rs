use crate::depot::heap::{self, hole};

pub(in crate::depot) mod direct;
pub(in crate::depot) mod stored;
pub(in crate::depot) mod vacancy;

/// One initialized position retaining its complete owning source.
pub struct Entry<S, V = ()> {
    pub(in crate::depot) owner: S,
    pub(in crate::depot) position: usize,
    pub(in crate::depot) next: V,
}

impl<S: hole::raw::Source, V> Entry<S, V> {
    /// # Safety
    /// The exact source exclusively owns this initialized position and accepts the replacement.
    /// Its range, ordering domain, and reverse map remain valid until this operation completes.
    pub(in crate::depot) unsafe fn new(owner: S, position: usize, next: V) -> Self {
        Self {
            owner,
            position,
            next,
        }
    }
}

impl<'a, S, V> Entry<&'a mut S, V> {
    pub(in crate::depot) fn into_parts(self) -> (Entry<&'a mut S>, V) {
        (
            Entry {
                owner: self.owner,
                position: self.position,
                next: (),
            },
            self.next,
        )
    }
}

impl<'a, S, K> Entry<&'a mut S, K> {
    pub(in crate::depot) fn prepare_key(
        self,
        priority: S::Priority,
    ) -> Entry<&'a mut S, heap::Stored<K, S::Priority>>
    where
        S: heap::raw::Store<Key = K>,
        K: crate::depot::raw::Address,
    {
        Entry {
            owner: self.owner,
            position: self.position,
            next: heap::Stored {
                identity: self.next,
                priority,
            },
        }
    }
}

impl<'a, S> Entry<&'a mut S> {
    pub(in crate::depot) fn first(owner: &'a mut S) -> Option<Self>
    where
        S: heap::raw::Store,
    {
        if hole::raw::Source::is_empty(&owner) {
            return None;
        }
        Some(Self {
            owner,
            position: 0,
            next: (),
        })
    }

    pub(in crate::depot) fn prepare(
        self,
        priority: S::Priority,
    ) -> Entry<&'a mut S, heap::Stored<S::Key, S::Priority>>
    where
        S: heap::raw::Store,
    {
        let key = self.value().identity;
        self.with_key(key).prepare_key(priority)
    }

    pub(in crate::depot) fn set_min_slot(self, priority: S::Priority) -> heap::Change
    where
        S: heap::raw::Store,
    {
        let entry = self.prepare(priority);
        if entry.is_root() {
            let change = if entry.value().priority == entry.next.priority {
                heap::Change::Contents
            } else {
                heap::Change::Minimum
            };
            heap::raw::Hole::prepare(entry);
            change
        } else {
            heap::raw::Hole::prepare(entry).change()
        }
    }

    pub(in crate::depot) fn retire_min(self) -> heap::Change
    where
        S: heap::raw::Store,
    {
        let change = self.change();
        heap::raw::Hole::retire(self);
        change
    }

    pub(in crate::depot) fn remove_slot(self) -> (heap::Vacancy<&'a mut S, S::Key>, S::Priority)
    where
        S: heap::raw::Store,
    {
        let vacant = heap::raw::Hole::remove(self);
        let key = vacant.value.identity;
        (
            heap::Vacancy {
                owner: vacant.owner,
                value: key,
            },
            vacant.value.priority,
        )
    }
}

impl<S, V> Entry<&mut S, V> {
    pub(in crate::depot) fn value(&self) -> &heap::Stored<S::Key, S::Priority>
    where
        S: heap::raw::Store,
    {
        // SAFETY: this entry retains one initialized position inside the owner's
        // heap prefix, which the store keeps initialized under this loan.
        unsafe {
            self.owner
                .values()
                .get_unchecked(self.position)
                .assume_init_ref()
        }
    }
}

impl<S, V> Entry<S, V> {
    pub(in crate::depot) fn change(&self) -> heap::Change {
        if self.is_root() {
            heap::Change::Minimum
        } else {
            heap::Change::Contents
        }
    }

    pub(in crate::depot) const fn is_root(&self) -> bool {
        self.position == 0
    }
}

impl<S, V> Entry<S, V> {
    pub(in crate::depot) fn into_inner(self) -> (S, usize, V) {
        (self.owner, self.position, self.next)
    }
}

impl<S> Entry<S> {
    pub(in crate::depot) fn with_key<K>(self, key: K) -> Entry<S, K> {
        Entry {
            owner: self.owner,
            position: self.position,
            next: key,
        }
    }
}

impl<S: hole::raw::Source> Entry<S> {
    /// Removes this member and returns its initialized owner and detached value.
    pub fn remove(self) -> (S, S::Value) {
        heap::raw::Hole::remove(self).into_inner()
    }

    /// Removes this exact initialized member and restores the retained heap.
    pub fn clear(self) -> S {
        heap::raw::Hole::retire(self)
    }
}
