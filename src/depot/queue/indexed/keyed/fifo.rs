use std::{marker, mem};

use crate::depot::{index::Initialized, queue::links};

/// One allocation-free, generation-checked intrusive FIFO.
/// It stores two `u32` endpoints; an invariant marker prevents widening branded keys.
#[repr(transparent)]
pub struct Fifo<K: crate::depot::raw::Key, P = ()> {
    pub(super) raw: links::Chain,
    invariant: Domain<K, P>,
}

impl<K: crate::depot::raw::Key, P> Default for Fifo<K, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: crate::depot::raw::Key, P> Fifo<K, P> {
    /// Consumes a retained nonempty source without repeating its admission.
    pub fn front<S: crate::depot::raw::KeySource<P, Key = K>>(
        source: crate::depot::raw::Proof<S, super::NonEmpty>,
    ) -> crate::depot::raw::Proof<(S, links::Index), Initialized<P>> {
        let source = source.into_inner();
        // SAFETY: the consumed source retains this exact nonempty chain and initialized endpoint.
        let index = unsafe { source.fifo().raw.front_key() };
        unsafe { crate::depot::raw::Proof::new((source, index)) }
    }

    pub const fn new() -> Self {
        Self {
            raw: links::Chain::new(),
            invariant: marker::PhantomData,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

const _: () = {
    assert!(
        mem::size_of::<Fifo<crate::depot::arena::slab::key::Handle>>() == 2 * mem::size_of::<u32>()
    );
};

impl<K: crate::depot::raw::Key, P> Fifo<K, P> {
    /// Retains the first initialized endpoint with its original FIFO source.
    pub fn first<S: crate::depot::raw::KeySource<P, Key = K>>(
        source: S,
    ) -> Option<crate::depot::raw::Proof<(S, links::Index), Initialized<P>>> {
        let index = source.fifo().raw.first()?;
        // SAFETY: this retained source owns the initialized endpoint and its exact plane.
        Some(unsafe { crate::depot::raw::Proof::new((source, index)) })
    }
}

impl<K: crate::depot::raw::Key, P> Fifo<K, P> {
    /// Appends the entire source in constant time and empties it.
    pub fn append<S: crate::depot::raw::KeyJoin<P, Key = K>>(source: S) {
        let Some(head) = Self::first(source) else {
            return;
        };
        let owner = head.source();
        let source = &owner.fifo().raw;
        let target = &owner.target().raw;
        // SAFETY: KeyJoin retains shared reads of both exact disjoint chains.
        let first = S::link(unsafe { head.as_ref() });
        let last = target.last().map(|index| {
            // SAFETY: Join retains the distinct destination's live endpoint.
            S::link(unsafe { crate::depot::raw::Proof::new((owner, index)) })
        });
        // SAFETY: Join retains these disjoint reciprocal chains and their exact endpoints.
        unsafe {
            crate::depot::raw::Splice::resolved(
                target,
                source,
                &first.raw,
                last.map(|link| &link.raw),
            )
        }
        .commit();
    }
}

type Domain<K, P> = marker::PhantomData<fn(K, P) -> (K, P)>;
