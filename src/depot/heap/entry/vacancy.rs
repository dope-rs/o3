use crate::depot::heap::{self, hole};

/// A bounded absent identity retaining its complete owning source and spare capacity.
pub struct Vacancy<S, V = ()> {
    pub(in crate::depot) owner: S,
    pub(in crate::depot) value: V,
}

impl<S: hole::raw::Source, V> Vacancy<S, V> {
    /// # Safety
    /// The exact source exclusively owns spare capacity, has length below u32::MAX,
    /// and accepts this bounded absent identity in its stable ordering domain.
    pub(in crate::depot) unsafe fn new(owner: S, value: V) -> Self {
        Self { owner, value }
    }
}

impl<S: hole::raw::Source> Vacancy<S, S::Value> {
    /// Inserts the admitted value and completes every ordering and reverse-map write.
    pub fn store(self) {
        heap::raw::Hole::append(self).commit::<true>();
    }
}

impl<'a, S, K> Vacancy<&'a mut S, K> {
    pub(in crate::depot) fn prepare(
        self,
        priority: S::Priority,
    ) -> Vacancy<&'a mut S, heap::Stored<K, S::Priority>>
    where
        S: heap::raw::Store<Key = K>,
        K: crate::depot::raw::Address,
    {
        Vacancy {
            owner: self.owner,
            value: heap::Stored {
                identity: self.value,
                priority,
            },
        }
    }
}

impl<S, V> Vacancy<S, V> {
    pub(in crate::depot) fn into_inner(self) -> (S, V) {
        (self.owner, self.value)
    }
}
