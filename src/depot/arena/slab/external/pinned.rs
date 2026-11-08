use std::{marker, pin::Pin};

use crate::depot::{
    arena::slab::{self, entries, external, storage},
    capacity,
};

pub(in crate::depot) type Inner<T, G, Tag> =
    slab::Pinned<external::Stamped<T, G>, Tag, 1, true, 1, storage::Lazy>;

pub struct Pinned<T, G: external::Generation, Tag = ()> {
    pub(in crate::depot) inner: Inner<T, G, Tag>,
    pub(in crate::depot) next: Option<G>,
}

pub type Anchor<'a, T, G, Tag = ()> = external::grant::Reservation<
    'a,
    slab::Anchor<'a, external::Stamped<T, G>, Tag, 1, true, 1, storage::Lazy>,
    G,
>;

pub type Anchored<'a, T, G, Tag = ()> =
    external::Occupied<slab::Anchored<'a, external::Stamped<T, G>, Tag, 1, true, 1, storage::Lazy>>;

impl<T, G: external::Generation, Tag> Pinned<T, G, Tag> {
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            inner: slab::Pinned {
                core: entries::Core::try_new(cap)?,
                _pin: marker::PhantomPinned,
                tag: marker::PhantomData,
            },
            next: Some(G::INITIAL),
        })
    }

    pub fn capacity(&self) -> capacity::Count {
        storage::Plane::count(&self.inner.core.slots)
    }

    pub fn try_grant(self: Pin<&mut Self>) -> Result<Anchor<'_, T, G, Tag>, slab::Vacancy> {
        let (next, inner) = self.split();
        external::grant::Reservation::try_new(next, || inner.vacant())
    }

    pub fn try_occupied(
        self: Pin<&mut Self>,
        key: external::Key<G, Tag>,
    ) -> Result<Anchored<'_, T, G, Tag>, external::errors::Lookup<G, Tag>> {
        let owner = self.into_ref().get_ref();
        let entry = entries::Occupied::try_at(&owner.inner.core, key.index())
            .map_err(|cause| external::errors::Lookup::Physical { key, cause })?
            .take();
        let actual = entry.value().generation;
        if actual != key.generation() {
            return Err(external::errors::Lookup::Generation { key, actual });
        }
        Ok(slab::Anchored {
            entry,
            tag: marker::PhantomData,
        }
        .into())
    }
}

impl<'a, T, G: external::Generation, Tag> Anchor<'a, T, G, Tag> {
    pub fn index(&self) -> u32 {
        self.inner.index()
    }

    pub fn occupy(self, value: T) -> (external::Key<G, Tag>, Pin<&'a mut T>) {
        let (inner, generation) = self.issue();
        let key = external::Key::new(inner.index(), generation);
        let (_, value) = inner.occupy(external::Stamped { generation, value });
        (key, value.pinned())
    }
}

impl<'a, T, G: external::Generation, Tag> Anchored<'a, T, G, Tag> {
    pub fn value(&mut self) -> Pin<&mut T> {
        self.inner.value().pinned()
    }

    pub fn into_value(self) -> Pin<&'a mut T> {
        self.inner.into_value().pinned()
    }

    pub fn discard(self) {
        self.inner.discard();
    }
}
