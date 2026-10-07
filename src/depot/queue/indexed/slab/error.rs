use crate::depot::{arena::slab::key, capacity, queue::indexed::slab};

/// The rejected handle and exact slot observation from its first lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<Tag = (), const MAX: u32 = { u32::MAX }, C = capacity::Count> {
    Index {
        key: slab::Handle<Tag, MAX>,
        capacity: C,
    },
    Uninitialized {
        key: slab::Handle<Tag, MAX>,
        initialized: u32,
    },
    Free {
        key: slab::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
    },
    Retired {
        key: slab::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
    },
    Generation {
        key: slab::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
    },
}
