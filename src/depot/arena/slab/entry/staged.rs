use std::marker;

use cycle::Policy;

use crate::{
    depot::arena::slab::{
        self, entries, external,
        key::{self, cycle},
    },
    mem::lease,
};

/// An initialized insertion that rolls back unless committed.
pub struct Staged<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<B, Tag, const ROOT: bool> Staged<B, Tag, ROOT> {
    pub(in crate::depot::arena::slab) fn with_state<S>(
        self,
        state: S,
    ) -> external::Staged<B, Tag, ROOT, S> {
        external::Staged {
            entry: self.entry,
            state,
            tag: self.tag,
        }
    }
}

impl<T, Tag, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, const ROOT: bool>
    slab::Staged<
        crate::mem::Lease<entries::Busy<'_, T, MAX, M, PARTITIONS>, lease::Open>,
        Tag,
        ROOT,
    >
{
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        let entry = self.entry.sink();
        key::Handle::new(entry.index(), entry.generation())
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.entry.parts_mut().0.value_mut()
    }

    pub fn publish(self) {
        drop(self.entry.into_parts().0);
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, const ROOT: bool>
    slab::Staged<
        crate::mem::Lease<
            entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>,
            lease::Open,
        >,
        Tag,
        ROOT,
    >
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn get(&self) -> &T {
        self.entry.sink().value()
    }

    pub fn commit(
        self,
    ) -> slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag, ROOT>
    {
        slab::Occupied {
            entry: self.take(),
            tag: marker::PhantomData,
        }
    }

    pub fn reject(self) -> T {
        self.take().remove()
    }

    fn take(self) -> entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS> {
        self.entry.into_parts().0
    }
}
