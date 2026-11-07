use std::convert;

use crate::depot::arena::slab::key;

/// The physical generation policy selected by a slab's owner.
pub struct Mode<const RECYCLE: bool>;

/// Exact exhaustion state of a physical generation policy.
pub trait Policy {
    type Stored<const MAX: u32>: Copy;
    type Retired: Copy + Eq;

    fn generation<const MAX: u32>(stored: Self::Stored<MAX>) -> key::Generation<MAX>;

    fn stored<const MAX: u32>(generation: key::Generation<MAX>) -> Self::Stored<MAX>;

    fn next<const MAX: u32>(current: Self::Stored<MAX>)
    -> Result<Self::Stored<MAX>, Self::Retired>;
}

impl Policy for Mode<false> {
    type Stored<const MAX: u32> = key::Generation<MAX>;
    type Retired = ();

    fn generation<const MAX: u32>(stored: Self::Stored<MAX>) -> key::Generation<MAX> {
        stored
    }

    fn stored<const MAX: u32>(generation: key::Generation<MAX>) -> Self::Stored<MAX> {
        generation
    }

    fn next<const MAX: u32>(
        current: Self::Stored<MAX>,
    ) -> Result<Self::Stored<MAX>, Self::Retired> {
        current.checked_add(1).ok_or(())
    }
}

impl Policy for Mode<true> {
    type Stored<const MAX: u32> = ();
    type Retired = convert::Infallible;

    fn generation<const MAX: u32>((): Self::Stored<MAX>) -> key::Generation<MAX> {
        key::Generation::MIN
    }

    fn stored<const MAX: u32>(_: key::Generation<MAX>) -> Self::Stored<MAX> {}

    fn next<const MAX: u32>((): Self::Stored<MAX>) -> Result<Self::Stored<MAX>, Self::Retired> {
        Ok(())
    }
}
