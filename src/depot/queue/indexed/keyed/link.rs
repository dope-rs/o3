use std::{marker, mem};

use crate::depot::queue::links;

/// One independently linkable hook tied to a complete generation-bearing key.
#[repr(transparent)]
pub struct Link<K: crate::depot::raw::Key, P = ()> {
    pub(super) raw: links::Hook,
    invariant: Domain<K, P>,
}

impl<K: crate::depot::raw::Key, P> Link<K, P> {
    /// Reports whether this link is attached to its one eligible queue.
    pub fn is_linked(&self, key: K) -> bool {
        self.raw.is_linked(key.index())
    }
}

impl<K: crate::depot::raw::Key, P> From<links::Index> for Link<K, P> {
    fn from(index: links::Index) -> Self {
        Self {
            raw: links::Hook::new(index),
            invariant: marker::PhantomData,
        }
    }
}

const _: () = {
    assert!(
        mem::size_of::<Link<crate::depot::arena::slab::key::Handle>>() == 2 * mem::size_of::<u32>()
    );
};

type Domain<K, P> = marker::PhantomData<fn(K, P) -> (K, P)>;
