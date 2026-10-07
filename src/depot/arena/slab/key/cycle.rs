use std::convert;

use crate::depot::arena::slab::key;

/// The physical generation policy selected by a slab's owner.
pub struct Mode<const RECYCLE: bool>;

/// Exact exhaustion state of a physical generation policy.
pub trait Policy {
    type Retired: Copy + Eq;

    fn next<const MAX: u32>(
        current: key::Generation<MAX>,
    ) -> Result<key::Generation<MAX>, Self::Retired>;
}

impl Policy for Mode<false> {
    type Retired = ();

    fn next<const MAX: u32>(
        current: key::Generation<MAX>,
    ) -> Result<key::Generation<MAX>, Self::Retired> {
        current.checked_add(1).ok_or(())
    }
}

impl Mode<true> {
    /// Advances a recyclable generation, wrapping at the domain maximum.
    pub(in crate::depot::arena::slab) fn advance<const MAX: u32>(
        current: key::Generation<MAX>,
    ) -> key::Generation<MAX> {
        current.checked_add(1).unwrap_or(key::Generation::MIN)
    }
}

impl Policy for Mode<true> {
    type Retired = convert::Infallible;

    fn next<const MAX: u32>(
        current: key::Generation<MAX>,
    ) -> Result<key::Generation<MAX>, Self::Retired> {
        Ok(Self::advance(current))
    }
}
