use std::convert;

use crate::depot::arena::slab::{external, lookup};

/// A rejected external identity with its complete slot observation.
#[derive(Debug)]
pub enum Lookup<G: external::Generation, Tag = (), B = convert::Infallible> {
    Physical {
        key: external::Key<G, Tag>,
        cause: lookup::Error<B, convert::Infallible>,
    },
    Generation {
        key: external::Key<G, Tag>,
        actual: G,
    },
}

/// The non-wrapping logical identity space has been permanently consumed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Exhausted;

const _: () = assert!(std::mem::size_of::<crate::depot::arena::slab::Vacancy>() == 1);
const _: () = assert!(std::mem::size_of::<Exhausted>() == 0);
