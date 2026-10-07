use std::marker;

use crate::depot::capacity;

/// An identity bounded by one invariant construction domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Key<'a, K> {
    key: K,
    brand: marker::PhantomData<fn(&'a ()) -> &'a ()>,
}

impl<'a, K> Key<'a, K> {
    pub(in crate::depot) fn new(proof: crate::depot::raw::Proof<K, capacity::Domain<'a>>) -> Self {
        let key = proof.into_inner();
        Self {
            key,
            brand: marker::PhantomData,
        }
    }

    pub(in crate::depot) fn into_inner(self) -> K {
        self.key
    }

    pub const fn as_inner(&self) -> &K {
        &self.key
    }
}
