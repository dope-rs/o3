use std::marker;

use crate::depot::{
    arena::slab::{self, key},
    capacity,
    queue::{links, round::set},
};

/// A resident retaining its order and a proved successor generation.
pub struct Reuse<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, const ROOT: bool = true, B = ()> {
    entry: slab::Reuse<'a, T, Tag, MAX, ROOT>,
    order: &'a set::Order<B>,
    index: links::Index,
}

impl<'a, T, Tag, const MAX: u32, const ROOT: bool, B>
    TryFrom<set::Occupied<'a, T, Tag, MAX, ROOT, B>> for Reuse<'a, T, Tag, MAX, ROOT, B>
{
    type Error = set::Occupied<'a, T, Tag, MAX, ROOT, B>;

    fn try_from(entry: Self::Error) -> Result<Self, Self::Error> {
        let set::Occupied {
            entry,
            order,
            index,
            phase,
        } = entry;
        match slab::Reuse::try_from(entry) {
            Ok(entry) => Ok(Self {
                entry,
                order,
                index,
            }),
            Err(entry) => Err(set::Occupied {
                entry,
                order,
                index,
                phase,
            }),
        }
    }
}

impl<'a, T, Tag, const MAX: u32, const ROOT: bool, B> Reuse<'a, T, Tag, MAX, ROOT, B> {
    pub fn replace(self, value: T) -> (set::Occupied<'a, T, Tag, MAX, ROOT, B>, T) {
        let (entry, prior) = self.entry.replace(value);
        (
            set::Occupied {
                entry,
                order: self.order,
                index: self.index,
                phase: marker::PhantomData,
            },
            prior,
        )
    }
}

impl<T, Tag, const MAX: u32, const ROOT: bool> Reuse<'_, T, Tag, MAX, ROOT> {
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        self.entry.key()
    }
}

impl<'h, T, Tag, const MAX: u32, const ROOT: bool>
    Reuse<'_, T, Tag, MAX, ROOT, capacity::Domain<'h>>
{
    pub fn key(&self) -> capacity::Key<'h, key::Handle<Tag, MAX>> {
        // SAFETY: the joint set allocated values and this order in the same domain.
        capacity::Key::new(unsafe { crate::depot::raw::Proof::new(self.entry.key()) })
    }
}
