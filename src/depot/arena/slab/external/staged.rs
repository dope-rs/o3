use std::marker;

use crate::{
    depot::arena::slab::{self, entries, external},
    mem::lease,
};

/// A logical identity projection of its native staged phase.
pub struct Staged<B, Tag = (), const ROOT: bool = true, S = ()> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) state: S,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool, S> Staged<B, Tag, ROOT, S> {
    pub(in crate::depot::arena::slab) fn into_native(self) -> slab::Staged<B, Tag, ROOT> {
        slab::Staged {
            entry: self.entry,
            tag: self.tag,
        }
    }
}

impl<T, G: external::Generation, Tag, const PHYSICAL_MAX: u32>
    external::Staged<
        crate::mem::Lease<
            entries::Busy<'_, external::Stamped<T, G>, PHYSICAL_MAX, entries::Interior<true>, 1>,
            lease::Open,
        >,
        Tag,
        true,
        G,
    >
{
    pub fn key(&self) -> external::Key<G, Tag> {
        external::Key::new(self.entry.sink().index(), self.state)
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.entry.parts_mut().0.value_mut().value
    }

    pub fn publish(self) {
        self.into_native().publish();
    }
}

impl<'a, T, G: external::Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    external::Staged<
        crate::mem::Lease<external::Busy<'a, T, G, PHYSICAL_MAX, PARTITIONS>, lease::Open>,
        Tag,
        true,
        external::Logical,
    >
{
    pub fn get(&self) -> &T {
        &self.entry.sink().value().value
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.entry.parts_mut().0.value_mut().value
    }

    pub fn publish(self) {
        self.into_native().publish();
    }

    pub fn commit(
        self,
    ) -> external::Occupied<external::Busy<'a, T, G, PHYSICAL_MAX, PARTITIONS>, Tag, true> {
        self.into_native().commit().into()
    }

    pub fn reject(self) -> T {
        self.into_native().reject().value
    }
}
