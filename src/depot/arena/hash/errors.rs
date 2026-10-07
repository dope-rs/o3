#[derive(Debug)]
pub enum Insert<T, Tag> {
    Full(T),
    Occupied {
        key: key::Handle<runtime::Domain<Tag>>,
        value: T,
    },
}

#[derive(Debug)]
pub enum Create<Tag> {
    Full {
        capacity: usize,
    },
    Occupied {
        key: key::Handle<runtime::Domain<Tag>>,
    },
}

#[derive(Debug)]
pub enum Lookup<Tag> {
    Index {
        key: key::Handle<runtime::Domain<Tag>>,
        capacity: usize,
    },
    Vacant {
        key: key::Handle<runtime::Domain<Tag>>,
        actual: key::Generation,
    },
    Generation {
        key: key::Handle<runtime::Domain<Tag>>,
        actual: key::Generation,
    },
}

use crate::depot::arena::{hash::runtime, slab::key};
