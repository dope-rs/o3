use std::mem;

use runtime::entry;

use crate::depot::{
    arena::{
        hash::{self, errors, runtime},
        slab::key,
    },
    map,
    queue::links,
};

/// A vacant value and directory bucket belonging to one exclusive owner.
pub struct Vacancy<'a, T, Tag> {
    value: hash::Slot<'a, T, Tag>,
    bucket: map::raw::Vacant,
    hash: u64,
}

const _: () = {
    assert!(mem::size_of::<Vacancy<'static, (), ()>>() == 4 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Option<Vacancy<'static, (), ()>>>()
            == mem::size_of::<Vacancy<'static, (), ()>>()
    );
};

impl<'a, T, Tag> Vacancy<'a, T, Tag> {
    pub(in crate::depot) fn new(
        entry: map::Vacancy<&'a mut hash::Core<T, Tag>>,
        hash: u64,
    ) -> Option<Self> {
        let (core, bucket, ()) = entry.into_parts();
        Some(Self {
            value: hash::Slot::new(core)?,
            bucket,
            hash,
        })
    }

    /// Returns the slot reserved for this insertion.
    ///
    /// This vacancy retains exclusive access until insertion or drop; the index
    /// alone does not establish an initialized value.
    pub fn index(&self) -> links::Index {
        self.value.index.into()
    }

    pub(in crate::depot) fn insert_entry(
        self,
        value: T,
    ) -> entry::Live<'a, T, Tag, entry::Indexed> {
        hash::Entry::index(self.value.insert(value), self.bucket, self.hash)
    }
}

impl<'a, T, Tag> entry::Vacancy<'a, T, Tag> {
    pub(in crate::depot) fn try_new(
        core: &'a mut hash::Core<T, Tag>,
        hash: u64,
        value: T,
        matches: impl Fn(&T, &T) -> bool,
    ) -> Result<(Self, T), errors::Insert<T, Tag>> {
        match map::raw::Probe::new(core, hash, |stored| matches(stored, &value)).entry() {
            map::Entry::Occupied(occupied) => Err(errors::Insert::Occupied {
                key: occupied.key(),
                value,
            }),
            map::Entry::Vacant(None) => Err(errors::Insert::Full(value)),
            map::Entry::Vacant(Some(place)) => Ok((place, value)),
        }
    }

    pub fn insert(self, value: T) -> key::Handle<runtime::Domain<Tag>> {
        self.insert_entry(value).key()
    }
}
