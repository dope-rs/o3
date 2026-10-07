pub use hasher::Hasher;
pub use plan::Plan;

use crate::{
    alloc,
    depot::{capacity, map},
};

mod hasher;
mod plan;

/// Failure to prove a map allocation shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capacity {
    Domain(capacity::Error),
    Layout {
        requested: usize,
        cause: alloc::Overflow,
    },
}

pub mod compact;
pub mod direct;
pub mod flat;
pub mod priority;
pub mod raw;
pub mod resident;

pub enum Entry<O, V> {
    Occupied(O),
    Vacant(V),
}

impl Capacity {
    pub const fn requested(self) -> usize {
        match self {
            Self::Domain(error) => error.requested(),
            Self::Layout {
                requested: value,
                cause: alloc::Overflow,
            } => value,
        }
    }
}

/// One occupied bucket retaining the exact query owner that proved it.
pub struct Found<Q> {
    owner: Q,
    bucket: map::raw::Occupied,
}

/// One vacant bucket retaining its exact directory owner.
pub struct Vacancy<Q, P = ()> {
    owner: Q,
    bucket: map::raw::Vacant,
    pub(in crate::depot::map) phase: P,
}

impl<Q> Found<Q> {
    pub(crate) fn new(proof: map::raw::Bucket<Q, map::raw::Occupied>) -> Self {
        let (owner, bucket) = proof.into_parts();
        Self { owner, bucket }
    }

    pub fn into_parts(self) -> (Q, map::raw::Occupied) {
        (self.owner, self.bucket)
    }

    pub(crate) fn read(&self) -> Found<&Q> {
        Found {
            owner: &self.owner,
            bucket: self.bucket,
        }
    }
}

impl<Q> Found<&mut Q> {
    pub(crate) fn reborrow(&mut self) -> Found<&mut Q> {
        Found {
            owner: &mut *self.owner,
            bucket: self.bucket,
        }
    }
}

impl<'a, Q> Found<&'a Q> {
    pub(crate) fn payload(&self) -> &'a Q::Payload
    where
        Q: map::raw::Query,
    {
        unsafe { self.owner.table().payload(&self.bucket) }
    }

    pub fn project(self) -> (&'a Q, &'a Q::Payload)
    where
        Q: map::raw::Query,
    {
        let payload = self.payload();
        (self.owner, payload)
    }
}

impl<Q> Found<Q> {
    pub fn copied(self) -> (Q, Q::Payload)
    where
        Q: map::raw::Query,
        Q::Payload: Copy,
    {
        let payload = *self.read().payload();
        (self.owner, payload)
    }
}

impl<Q, P> Vacancy<Q, P> {
    pub(crate) fn with_phase<N>(self, phase: N) -> Vacancy<Q, N> {
        Vacancy {
            owner: self.owner,
            bucket: self.bucket,
            phase,
        }
    }

    pub(crate) fn source_mut(&mut self) -> &mut Q {
        &mut self.owner
    }

    pub fn into_parts(self) -> (Q, map::raw::Vacant, P) {
        (self.owner, self.bucket, self.phase)
    }

    pub(crate) fn source(&self) -> &Q {
        &self.owner
    }
}

impl<Q> Vacancy<Q> {
    pub(crate) fn new(proof: map::raw::Bucket<Q, map::raw::Vacant>) -> Self {
        let (owner, bucket) = proof.into_parts();
        Self {
            owner,
            bucket,
            phase: (),
        }
    }
}
