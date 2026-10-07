mod raw;

use std::{mem, num::NonZero, ops};

use crate::{
    depot,
    depot::{
        array::Index,
        queue::{
            indexed::lanes::{
                entry::{Error, State},
                runtime,
            },
            links, tagged,
            tagged::entry,
        },
        raw::Live,
    },
};

/// A represented endpoint tied to its immutable owner.
/// `Initialized<Endpoint<&S, Option<links::Index>>>` names this owner's head below `u32::MAX`.
/// The full source loan preserves the head, topology and every reachable initialized hook.
pub struct Endpoint<O, K = (), const INITIALIZED: bool = true> {
    store: O,
    endpoint: NonZero<u32>,
    key: K,
}

impl<O: ops::Deref> Endpoint<O>
where
    O::Target: crate::depot::raw::TagStore,
{
    /// # Safety
    /// The retained endpoint proof was constructed for this exact immutable store.
    pub(in crate::depot) unsafe fn new(store: O, endpoint: NonZero<u32>) -> Self {
        Self {
            store,
            endpoint,
            key: (),
        }
    }

    pub fn front(self) -> Option<Live<Self, links::Index, links::Chain>> {
        let index = tagged::raw::Head::first(self.head())?;
        // SAFETY: this owner's private head proves live membership in this exact lane.
        Some(unsafe { crate::depot::raw::Proof::new((self, index)) })
    }

    pub fn is_empty(&self) -> bool {
        tagged::raw::Head::first(self.head()).is_none()
    }

    pub(in crate::depot::queue::tagged) fn head(
        &self,
    ) -> &<O::Target as crate::depot::raw::TagStore>::Head {
        let index = (u32::MAX - self.endpoint.get()) as usize;
        // SAFETY: each constructor binds the endpoint to this exact immutable owner.
        unsafe {
            crate::depot::raw::TagStore::heads(&*self.store)
                .as_ref()
                .get_unchecked(index)
        }
    }

    pub(in crate::depot) const fn endpoint(&self) -> NonZero<u32> {
        self.endpoint
    }
}

impl<'a, S: crate::depot::raw::TagStore> Endpoint<&'a S> {
    pub fn into_store(self) -> &'a S {
        self.store
    }
}

impl<'a, S: crate::depot::raw::LaneStore> IntoIterator for Endpoint<&'a S> {
    type Item = Live<&'a S, links::Index, links::Chain>;
    type IntoIter = Endpoint<&'a S, Option<links::Index>>;

    fn into_iter(self) -> Self::IntoIter {
        // SAFETY: LaneStore retains linked nodes throughout the immutable source loan.
        let index = (u32::MAX - self.endpoint.get()) as usize;
        unsafe { depot::raw::Project::<usize, Self::IntoIter>::project(self.store, index) }
    }
}

impl<'a, S: crate::depot::raw::TagStore> Iterator for Endpoint<&'a S, Option<links::Index>> {
    type Item = Live<&'a S, links::Index, links::Chain>;

    fn next(&mut self) -> Option<Self::Item> {
        use tagged::hook::Link;

        let index = self.key?;
        // SAFETY: the immutable lane retains this live node and its reciprocal successor.
        let link = unsafe { self.store.hook(index) }.next.get();
        let endpoint = Link::endpoint(self.endpoint);
        self.key = (link != endpoint)
            .then(|| unsafe { links::Index::new_raw(depot::raw::EndpointWord::raw(link)) });
        // SAFETY: the admitted cursor retains this exact owner's initialized hook.
        Some(unsafe { depot::raw::Proof::new((self.store, index)) })
    }
}

impl<O: ops::Deref, K, const INITIALIZED: bool> AsRef<O::Target> for Endpoint<O, K, INITIALIZED>
where
    O::Target: crate::depot::raw::TagStore,
{
    fn as_ref(&self) -> &O::Target {
        &self.store
    }
}

impl<O: ops::DerefMut, K, const INITIALIZED: bool> Endpoint<O, K, INITIALIZED>
where
    O::Target: crate::depot::raw::TagStore,
{
    /// # Safety
    /// The node belongs to this exact exclusively borrowed owner and its payload
    /// initialization matches READY.
    pub(in crate::depot) unsafe fn at<const READY: bool>(
        self,
        key: links::Index,
    ) -> Endpoint<O, links::Index, READY> {
        Endpoint {
            store: self.store,
            endpoint: self.endpoint,
            key,
        }
    }
}

impl<O: ops::DerefMut> Endpoint<O>
where
    O::Target: crate::depot::raw::TagStore,
{
    pub(in crate::depot) fn reborrow_mut(&mut self) -> Endpoint<&mut O::Target> {
        Endpoint {
            store: &mut *self.store,
            endpoint: self.endpoint,
            key: (),
        }
    }
}

impl<O: ops::Deref, const N: usize> Endpoint<O>
where
    O::Target: crate::depot::raw::TagStore<Heads = [links::Chain; N]>,
{
    pub fn constant<const L: usize>(store: O) -> Self {
        const { assert!(L < N && N <= u32::MAX as usize) };
        Self {
            store,
            // SAFETY: the const proof and the owner's array share the same N.
            endpoint: const { NonZero::<u32>::MIN.saturating_add(u32::MAX - 1 - L as u32) },
            key: (),
        }
    }

    pub(in crate::depot) fn indexed(store: O, lane: Index<N>) -> Self {
        Self {
            store,
            // SAFETY: Index<N> excludes N and the owner's heads have exactly N entries.
            endpoint: unsafe { NonZero::new_unchecked(u32::MAX - lane.index() as u32) },
            key: (),
        }
    }

    pub(in crate::depot) fn select<I, const M: usize>(
        mut store: O,
        order: [(Index<N>, I); M],
    ) -> Option<(I, Live<O, links::Index, links::Chain>)>
    where
        O: ops::DerefMut,
    {
        for (lane, item) in order {
            let lane = Self::indexed(store, lane);
            if let Some(index) = tagged::raw::Head::first(lane.head()) {
                // SAFETY: the private head proves live membership under the retained owner.
                return Some((item, unsafe {
                    crate::depot::raw::Proof::new((lane.store, index))
                }));
            }
            store = lane.store;
        }
        None
    }
}

const _: () = assert!(mem::size_of::<NonZero<u32>>() == mem::size_of::<u32>());

impl<S: crate::depot::raw::LaneStore> Endpoint<&mut S> {
    pub fn pop_front(&mut self) -> Option<S::Item> {
        self.pop_entry().map(|(_, value)| value)
    }

    pub fn pop_entry(&mut self) -> Option<(usize, S::Item)> {
        let index = tagged::raw::Head::first(self.head())?;
        let endpoint = self.endpoint;
        // SAFETY: the exact endpoint's current front retains its exclusive owner through removal.
        let vacant = unsafe {
            tagged::Linked::new(self.reborrow_mut().at::<true>(index)).unlink_front(endpoint)
        };
        Some((index.get(), unsafe { vacant.take() }))
    }
}

impl<T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    tagged::Endpoint<&mut runtime::Fifo<T, C, L, H>>
{
    pub fn entry(&mut self, index: usize) -> Result<Entry<'_, T, C, L, H>, Error<C>> {
        let capacity = self.as_ref().capacity();
        if index >= capacity.count().get() {
            return Err(Error { index, capacity });
        }
        // SAFETY: the retained lane owner proved this initialized hook's bound.
        let index = unsafe { links::Index::new_usize(index) };
        if unsafe { depot::raw::LaneStore::nodes(self.as_ref()).get_unchecked(index.get()) }
            .hook
            .is_linked(index)
        {
            let node =
                unsafe { depot::raw::LaneStore::nodes(self.as_ref()).get_unchecked(index.get()) };
            return Ok(State::Occupied(unsafe {
                (*node.value.get()).assume_init_ref()
            }));
        }
        let entry = unsafe { self.reborrow_mut().at::<false>(index) };
        Ok(State::Vacant(unsafe { tagged::Vacant::new(entry) }))
    }

    pub fn vacant(&mut self, index: usize) -> Result<Vacancy<'_, T, C, L, H>, entry::Error<C>> {
        let capacity = self.as_ref().capacity();
        if index >= capacity.count().get() {
            return Err(entry::Error::Index { index, capacity });
        }
        // SAFETY: the original exclusive owner initializes every node within capacity.
        let entry = unsafe {
            self.reborrow_mut()
                .at::<false>(links::Index::new_usize(index))
        };
        tagged::Vacant::try_new::<C>(entry)
    }
}

impl<S: crate::depot::raw::TagStore, const INITIALIZED: bool>
    Endpoint<&mut S, links::Index, INITIALIZED>
{
    pub(in crate::depot) fn parts(&self) -> (Endpoint<&S>, links::Index) {
        (
            Endpoint {
                store: &*self.store,
                endpoint: self.endpoint,
                key: (),
            },
            self.key,
        )
    }
}

/// A vacant lane slot keeps its unique runtime FIFO loan until publication.
type Vacancy<'a, T, C, L, H> =
    tagged::Vacant<tagged::Endpoint<&'a mut runtime::Fifo<T, C, L, H>, links::Index, false>>;

/// A lane observation keeps either the initialized value or its vacant publication capability.
type Entry<'a, T, C, L, H> = State<&'a T, Vacancy<'a, T, C, L, H>>;
