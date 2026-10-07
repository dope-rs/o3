use crate::depot::{
    index::Initialized,
    queue::{indexed::lanes, links, tagged},
};

// SAFETY: the exclusive borrow retains the original capability and all of its projections.
unsafe impl<E: crate::depot::raw::TagEntry> crate::depot::raw::TagEntry for &mut E {
    const INITIALIZED: bool = E::INITIALIZED;
    type Store = E::Store;

    fn store(&self) -> &Self::Store {
        E::store(self)
    }

    fn hook(&self) -> &tagged::Hook {
        E::hook(self)
    }

    fn index(&self) -> links::Index {
        E::index(self)
    }
}

// SAFETY: the membership proof retains its initialized node and complete exclusive storage borrow.
unsafe impl<S: crate::depot::raw::TagStore> crate::depot::raw::TagEntry
    for crate::depot::raw::Proof<(&mut S, links::Index), Initialized<links::Chain>>
{
    type Store = S;

    fn store(&self) -> &S {
        self.source()
    }
    fn hook(&self) -> &tagged::Hook {
        unsafe { self.source().hook(*self.key()) }
    }
    fn index(&self) -> links::Index {
        *self.key()
    }
}
// SAFETY: ordinary membership has no rollback obligation.
unsafe impl<E: crate::depot::raw::TagEntry> crate::depot::raw::TagFinish<E> for () {
    type Storage = E;

    fn new(entry: E) -> Self::Storage {
        entry
    }

    fn entry(storage: &Self::Storage) -> &E {
        storage
    }

    fn entry_mut(storage: &mut Self::Storage) -> &mut E {
        storage
    }

    fn into_inner(storage: Self::Storage) -> E {
        storage
    }
}

// SAFETY: the position owns the entry unchanged; it cannot select a different store.
unsafe impl<E: crate::depot::raw::TagEntry, const N: usize> crate::depot::raw::TagEntry
    for tagged::Position<E, N>
{
    const INITIALIZED: bool = E::INITIALIZED;
    type Store = E::Store;

    fn store(&self) -> &Self::Store {
        self.entry().store()
    }

    fn hook(&self) -> &tagged::Hook {
        self.entry().hook()
    }

    fn index(&self) -> links::Index {
        self.entry().index()
    }
}

// SAFETY: the selected Index<N> and the entry owner's head array share N in this bound.
unsafe impl<E: crate::depot::raw::TagEntry, const N: usize> crate::depot::raw::TagTarget
    for tagged::Position<E, N>
where
    E::Store: crate::depot::raw::TagStore<Heads = [links::Chain; N]>,
{
    fn lane(&self) -> tagged::Endpoint<&Self::Store> {
        self.destination()
    }
}

// SAFETY: a keyed lane is created only from its exact admitted node and unique owner.
unsafe impl<S: crate::depot::raw::TagStore, const INITIALIZED: bool> crate::depot::raw::TagEntry
    for tagged::Endpoint<&mut S, links::Index, INITIALIZED>
{
    const INITIALIZED: bool = INITIALIZED;
    type Store = S;
    fn store(&self) -> &Self::Store {
        self.as_ref()
    }
    fn index(&self) -> links::Index {
        self.parts().1
    }
    fn hook(&self) -> &tagged::Hook {
        unsafe { crate::depot::raw::TagStore::hook(self.as_ref(), self.parts().1) }
    }
}

// SAFETY: the endpoint and node retain the same original unique owner.
unsafe impl<S: crate::depot::raw::TagStore, const INITIALIZED: bool> crate::depot::raw::TagTarget
    for tagged::Endpoint<&mut S, links::Index, INITIALIZED>
{
    fn lane(&self) -> tagged::Endpoint<&Self::Store> {
        self.parts().0
    }
}

// SAFETY: the exclusive storage loan retains the validated node and excludes all competing mutation.
unsafe impl<T, const N: usize, const L: usize, C: crate::depot::raw::Count, const INITIALIZED: bool>
    crate::depot::raw::TagEntry for lanes::Position<'_, T, N, L, C, INITIALIZED>
{
    const INITIALIZED: bool = INITIALIZED;
    type Store = lanes::Fifo<T, N, C>;
    fn store(&self) -> &Self::Store {
        self.parts().0
    }
    fn index(&self) -> links::Index {
        self.parts().1
    }
    fn hook(&self) -> &tagged::Hook {
        use crate::depot::raw::LaneStore as _;
        &unsafe { self.parts().0.nodes().get_unchecked(self.parts().1.get()) }.hook
    }
}

// SAFETY: the constant lane is projected from the entry's same N-head owner.
unsafe impl<T, const N: usize, const L: usize, C: crate::depot::raw::Count, const INITIALIZED: bool>
    crate::depot::raw::TagTarget for lanes::Position<'_, T, N, L, C, INITIALIZED>
{
    fn lane(&self) -> tagged::Endpoint<&Self::Store> {
        tagged::Endpoint::constant::<L>(self.parts().0)
    }
}
