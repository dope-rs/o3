use std::marker;

use cycle::Policy;

use crate::depot::arena::slab::{
    entries,
    key::{self, cycle},
    lookup, storage,
};

/// Read access to the slots disjoint from one retained exclusive slot.
pub struct Rest<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
    P = (),
    S = storage::Heap,
    I = storage::Eager,
> where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    core: &'a entries::Core<T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, S, I>,
    excluded: entries::SlotIndex,
    plane: Domain<Tag, P>,
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, P, S, I>
    Rest<'a, T, Tag, MAX, RECYCLE, PARTITIONS, P, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot::arena::slab) fn new(
        busy: &entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P, S, I>,
    ) -> Self {
        Self {
            core: busy.core,
            excluded: entries::SlotIndex(busy.index()),
            plane: marker::PhantomData,
        }
    }

    pub fn try_get(
        &self,
        key: key::Handle<Tag, MAX>,
    ) -> Result<&'a T, key::Error<Tag, MAX, lookup::Busy, <cycle::Mode<RECYCLE> as Policy>::Retired>>
    {
        let entry = entries::Occupied::try_key(self.core, key).map_err(|error| match error {
            key::Error::Index { key, capacity } => key::Error::Index { key, capacity },
            key::Error::Free { key, actual } => key::Error::Free { key, actual },
            key::Error::Reserved { key, actual } => key::Error::Reserved { key, actual },
            key::Error::Retired {
                key,
                actual,
                retired,
            } => key::Error::Retired {
                key,
                actual,
                retired,
            },
            key::Error::Generation { key, actual } => key::Error::Generation { key, actual },
        })?;
        if key.index() == self.excluded.get() {
            return Err(key::Error::Busy {
                key,
                actual: key.generation(),
                borrow: lookup::Busy {
                    index: self.excluded.get(),
                },
            });
        }
        Ok(entry.read())
    }
}

type Domain<Tag, P> = marker::PhantomData<fn(Tag, P) -> (Tag, P)>;
