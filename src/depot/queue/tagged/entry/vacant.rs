use tagged::entry;

use crate::depot::{
    array::Index,
    queue::{links, tagged},
};

/// Exclusive detached membership of an initialized entry.
pub struct Vacant<E: crate::depot::raw::TagEntry>(E);

impl<E: crate::depot::raw::TagEntry> Vacant<E> {
    /// # Safety
    /// The entry has just been initialized detached or consumed from Linked::unlink.
    pub(in crate::depot) unsafe fn new(entry: E) -> Self {
        Self(entry)
    }

    pub(in crate::depot) fn try_new<C>(entry: E) -> Result<Self, entry::Error<C>> {
        use tagged::hook::Link;

        let index = entry.index();
        let hook = entry.hook();
        let previous = hook.previous.get();
        if previous != Link::node(index) {
            let cause = entry::Error::Linked {
                index,
                previous: crate::depot::raw::EndpointWord::raw(previous),
                next: crate::depot::raw::EndpointWord::raw(hook.next.get()),
            };
            return Err(cause);
        }
        Ok(Self(entry))
    }

    pub fn entry(&self) -> &E {
        &self.0
    }

    pub fn into_inner(self) -> E {
        self.0
    }

    pub fn select<const N: usize>(self, lane: Index<N>) -> Vacant<tagged::Position<E, N>>
    where
        E::Store: crate::depot::raw::TagStore<Heads = [links::Chain; N]>,
    {
        Vacant(tagged::Position::new(self.0, lane))
    }

    pub fn detach(entry: E) -> Self {
        use tagged::hook::Link;

        if entry.hook().previous.get() == Link::node(entry.index()) {
            return Self(entry);
        }
        // SAFETY: private Hook transitions admit exactly detached or reciprocal linked membership.
        unsafe { tagged::Linked::new(entry) }.unlink()
    }
}

impl<E: crate::depot::raw::TagTarget> Vacant<E> {
    pub fn push<const FRONT: bool>(self) -> tagged::Linked<E> {
        const { assert!(E::INITIALIZED) };
        use tagged::hook::Link;

        let lane = self.0.lane();
        let index = self.0.index();
        let hook = self.0.hook();
        let endpoint = Link::endpoint(lane.endpoint());
        let head = lane.head();
        let adjacent = tagged::raw::Head::adjacent::<FRONT>(head).map(|index| {
            // SAFETY: the retained lane owns this initialized endpoint.
            (
                index,
                unsafe { crate::depot::raw::TagStore::hook(lane.as_ref(), index) }.parts(),
            )
        });
        // SAFETY: vacancy and its selected endpoint retain this exact reciprocal lane.
        unsafe {
            hook.parts()
                .push::<FRONT, _>(head, index, endpoint, adjacent)
        };
        // SAFETY: the detached entry was inserted exactly once into its own lane.
        unsafe { tagged::Linked::new(self.0) }
    }
}

impl<E: crate::depot::raw::TagEntry> tagged::Vacant<E>
where
    E::Store: crate::depot::raw::LaneStore,
{
    /// # Safety
    /// This vacancy owns the initialized payload just detached from its lane.
    pub(in crate::depot) unsafe fn take(self) -> <E::Store as crate::depot::raw::LaneStore>::Item {
        unsafe {
            (*<E::Store as crate::depot::raw::LaneStore>::node(self.entry())
                .value
                .get())
            .assume_init_read()
        }
    }

    /// # Safety
    /// This vacancy was obtained by detaching an initialized resident.
    pub(in crate::depot) unsafe fn retire(self) {
        unsafe {
            (*<E::Store as crate::depot::raw::LaneStore>::node(self.entry())
                .value
                .get())
            .assume_init_drop()
        };
    }
}

impl<E: crate::depot::raw::LaneInit> tagged::Vacant<E>
where
    E::Store: crate::depot::raw::LaneStore,
{
    pub fn insert<const FRONT: bool>(
        self,
        value: <E::Store as crate::depot::raw::LaneStore>::Item,
    ) -> tagged::Linked<E::Ready, <E::Store as crate::depot::raw::LaneStore>::Publish<E::Ready>>
    {
        // SAFETY: vacancy retains the exact exclusive payload and protects its hook.
        unsafe {
            (*<E::Store as crate::depot::raw::LaneStore>::node(self.entry())
                .value
                .get())
            .write(value)
        };
        let entry = unsafe { self.into_inner().ready() };
        unsafe { tagged::Vacant::new(entry) }
            .push::<FRONT>()
            .into_policy()
    }
}
