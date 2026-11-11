use crate::{
    depot::arena::slab::{self, entries, external},
    mem::lease,
};

/// A logical identity projection of its native staged phase.
pub type Staged<B, Tag = (), const ROOT: bool = true> =
    external::Occupied<slab::Staged<B, Tag, ROOT>>;

impl<T, G: external::Generation, Tag, M: entries::Mode, const N: usize>
    external::Staged<crate::mem::Lease<external::Busy<'_, T, G, M, N>, lease::Open>, Tag, true>
{
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.get_mut().value
    }

    pub fn publish(self) {
        self.inner.publish();
    }
}

impl<'a, T, G: external::Generation, Tag, const PARTITIONS: usize>
    external::StagedSlot<'a, T, G, Tag, PARTITIONS>
{
    pub fn get(&self) -> &T {
        &self.inner.get().value
    }

    pub fn commit(self) -> external::OccupiedSlot<'a, T, G, Tag, PARTITIONS> {
        self.inner.commit().into()
    }

    pub fn reject(self) -> T {
        self.inner.reject().value
    }
}
