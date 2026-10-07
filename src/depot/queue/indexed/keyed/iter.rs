use std::marker;

use crate::depot::{index::Initialized, queue::links};

/// Each yielded slot keeps its original source and link plane borrowed.
pub struct Iter<S: crate::depot::raw::KeyRead<P>, P = (), const REVERSE: bool = false> {
    source: S,
    next: Option<links::Index>,
    plane: marker::PhantomData<fn(P) -> P>,
}

impl<S: crate::depot::raw::KeyRead<P>, P, const REVERSE: bool> Iter<S, P, REVERSE> {
    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn from_live(live: crate::depot::raw::Proof<(S, links::Index), Initialized<P>>) -> Self {
        let (source, index) = live.into_inner();
        Self {
            source,
            next: Some(index),
            plane: marker::PhantomData,
        }
    }

    pub fn new(source: S) -> Self {
        let chain = &source.fifo().raw;
        let next = if REVERSE { chain.last() } else { chain.first() };
        Self {
            source,
            next,
            plane: marker::PhantomData,
        }
    }

    pub fn next_entry(
        &mut self,
    ) -> Option<crate::depot::raw::Proof<(&S, links::Index), Initialized<P>>> {
        let index = self.next?;
        // SAFETY: the source has retained this endpoint or its predecessor's neighbor.
        let entry = S::entry(unsafe { crate::depot::raw::Proof::new((&self.source, index)) });
        self.next = if REVERSE {
            entry.predecessor()
        } else {
            entry.successor()
        };
        // SAFETY: advancing only the iterator leaves the same source and slot intact.
        Some(unsafe { crate::depot::raw::Proof::new((&self.source, index)) })
    }
}

impl<S: crate::depot::raw::KeyRead<P>, P> Iter<S, P> {
    /// Advances a retained membership without another keyed lookup.
    pub fn successor(
        live: crate::depot::raw::Proof<(S, links::Index), Initialized<P>>,
    ) -> Option<crate::depot::raw::Proof<(S, links::Index), Initialized<P>>> {
        // SAFETY: KeyRead preserves this exact member and permits only shared traversal.
        let next = S::entry(unsafe { live.as_ref() }).successor()?;
        let (source, _) = live.into_inner();
        // SAFETY: Read retains this member, its linked successor, and their source.
        Some(unsafe { crate::depot::raw::Proof::new((source, next)) })
    }
}
