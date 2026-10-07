//! Growable payloads with independent indexed FIFO memberships.

use std::{array, iter, mem, num::NonZero, ops};

pub use entry::Entry;
pub use error::Error;

use crate::{
    alloc::{self, BoxSliceExt as _, VecExt as _, raw::VecExt as _},
    base::marker::ThreadBound,
    depot::{
        capacity,
        heap::indexed,
        queue::{
            indexed::{lanes::runtime, planes},
            links,
        },
    },
};

pub mod bound;
mod entry;
pub mod groups;
pub mod resident;
pub use state::State;

mod error;
pub mod map;
pub mod phase;

/// Statically builds one payload from its exact topology index.
pub trait Seed<T> {
    fn seed(&self, index: links::Index) -> T;
}

impl<T: Default> Seed<T> for () {
    fn seed(&self, _: links::Index) -> T {
        T::default()
    }
}

/// Coupled minimum priorities and their exact payload domain.
pub trait Priority<T, P: Ord, const PLANES: usize, C: alloc::raw::Extent>: Sized {
    fn try_seed(len: C, seed: impl Seed<T>) -> Result<Self, Error<C>>;
    fn try_repeat(len: C, priority: P) -> Result<Self, Error<C>>
    where
        T: Default,
        P: Copy;
    fn reset<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a;
    fn heap(&self) -> &indexed::Min<P, C>;
    fn select(&mut self, index: usize) -> Option<(indexed::Entry<'_, P, C>, Entry<'_, T, PLANES>)>;
    fn first(&mut self) -> Option<(indexed::Occupied<'_, P, C>, Entry<'_, T, PLANES>)>;
    fn try_grow_to(&mut self, len: C, seed: impl Seed<T>) -> Result<ops::Range<usize>, Error<C>>;
}

mod state;

#[repr(C)]
struct Node<T, const PLANES: usize> {
    value: T,
    hooks: [links::Hook; PLANES],
}

impl<T, const PLANES: usize> Node<T, PLANES> {
    fn new(index: links::Index, value: T) -> Self {
        Self {
            value,
            hooks: array::from_fn(|_| links::Hook::new(index)),
        }
    }
}

struct Hooks<'a, T, const PLANES: usize, const PLANE: usize>(&'a [Node<T, PLANES>]);

// SAFETY: each index maps to its one co-located hook in the const-selected
// plane, entries never change logical index, and vector growth cannot
// invalidate index links.
unsafe impl<T, const PLANES: usize, const PLANE: usize> crate::depot::raw::Hooks
    for Hooks<'_, T, PLANES, PLANE>
{
    unsafe fn hook_raw(&self, index: links::Index) -> &'_ links::Hook {
        const { assert!(PLANE < PLANES, "plane") };
        // SAFETY: HookStore's caller proves this compact index is represented.
        let entry = unsafe { self.0.get_unchecked(index.get()) };
        &entry.hooks[PLANE]
    }
}

/// Growable indexed values with independent FIFO membership planes.
pub struct Set<T, const PLANES: usize, A = ()> {
    core: Core<T, PLANES>,
    index: A,
}

struct Core<T, const PLANES: usize> {
    entries: Vec<Node<T, PLANES>>,
    queues: [links::Chain<u32>; PLANES],
    _thread: ThreadBound,
}

/// Exclusive access to one validated payload and all of its queue hooks.
impl<T, const PLANES: usize> Set<T, PLANES> {
    pub const ENTRY_BYTES: usize = mem::size_of::<Node<T, PLANES>>();

    pub fn try_seed<S: planes::Seed<T>>(len: usize, seed: S) -> Result<Self, alloc::Error> {
        let count = capacity::Count::try_count(len, alloc::Error::Overflow)?;
        let len = count.get();
        let entries = Box::try_from_fn(len, |index| {
            // SAFETY: the admitted capacity covers every initialized compact index.
            let index = unsafe { links::Index::new_usize(index) };
            Node::new(index, seed.seed(index))
        })?
        .into_vec();
        Ok(Self {
            core: Core {
                entries,
                queues: [const { links::Chain::new() }; PLANES],
                _thread: ThreadBound::NEW,
            },
            index: (),
        })
    }
}

impl<T, const PLANES: usize, A> Set<T, PLANES, A> {
    /// Exclusively borrows the payload and hooks at `index`.
    pub fn entry(&mut self, index: usize) -> Option<Entry<'_, T, PLANES>> {
        if index >= self.len() {
            return None;
        }
        // SAFETY: Set construction caps its length and `index` is in bounds.
        let index = unsafe { links::Index::new_usize(index) };
        Some(Entry::new(&mut self.core, index))
    }

    /// Borrows one plane in FIFO order without changing its membership.
    pub fn iter<const PLANE: usize>(&self) -> impl Iterator<Item = (usize, &T)> + '_ {
        self.core
            .iter::<PLANE>()
            .map(|(index, value)| (index.get(), value))
    }

    /// Exclusively binds the oldest payload to its exact membership proof.
    pub fn front_entry<const PLANE: usize>(
        &mut self,
    ) -> Option<Entry<'_, T, PLANES, phase::Front<PLANE>>> {
        self.core.front_entry::<PLANE>()
    }

    pub fn len_in<const PLANE: usize>(&self) -> usize {
        const { assert!(PLANE < PLANES) };
        self.core.queues[PLANE].len()
    }

    pub fn clear<const PLANE: usize>(&mut self) {
        self.core.clear::<PLANE>();
    }
}

impl<T, const PLANES: usize> Set<T, PLANES> {
    /// Grows to `len`, preserving every existing index and membership.
    /// Allocation failure leaves the original logical length and topology unchanged.
    pub fn try_grow_to<S: planes::Seed<T>>(
        &mut self,
        len: usize,
        seed: S,
    ) -> Result<ops::Range<usize>, crate::alloc::Error> {
        let count = capacity::Count::try_count(len, alloc::Error::Overflow)?;
        let old = self.core.entries.len();
        let core = self.core.grow(count.raw(), seed)?;
        Ok(old..core.entries.len())
    }
}

impl<T, const PLANES: usize> Core<T, PLANES> {
    fn grow<S: planes::Seed<T>>(
        &mut self,
        len: u32,
        seed: S,
    ) -> Result<&mut Self, crate::alloc::Error> {
        let len = len as usize;
        let original = self.entries.len();
        if len <= original {
            return Ok(self);
        }
        self.entries.try_grow(len - original)?;
        for index in original..len {
            // SAFETY: admitted capacity covers this compact index and its next vacant slot.
            unsafe {
                let compact = links::Index::new_usize(index);
                let value = seed.seed(compact);
                self.entries.vacancy().insert(Node::new(compact, value));
            }
        }
        Ok(self)
    }
}

impl<T, const PLANES: usize, A> Set<T, PLANES, A> {
    /// Selects a payload and its admitted compact identity in one lookup.
    pub fn resolve(&self, index: usize) -> Option<(NonZero<u32>, &T)> {
        self.core
            .resolve(index)
            .map(|(index, value)| (index.position(), value))
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.core.entries.get(index).map(|node| &node.value)
    }
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = &mut T> + DoubleEndedIterator {
        self.core.entries.iter_mut().map(|node| &mut node.value)
    }
    /// Returns retained physical rows, including rows outside the active prefix.
    pub fn len(&self) -> usize {
        self.core.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.core.entries.is_empty()
    }
}

const _: () = {
    assert!(mem::size_of::<Entry<'static, (), 1>>() == 2 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Entry<'static, (), 1, phase::Front<0>>>() == 2 * mem::size_of::<usize>()
    );
};

impl<T, const PLANES: usize> Core<T, PLANES> {
    fn resolve(&self, index: usize) -> Option<(links::Index, &T)> {
        let entry = self.entries.get(index)?;
        // SAFETY: construction excludes the sentinel and this index is represented.
        let index = unsafe { links::Index::new_usize(index) };
        Some((index, &entry.value))
    }

    fn iter<const PLANE: usize>(&self) -> impl Iterator<Item = (links::Index, &T)> + '_ {
        const { assert!(PLANE < PLANES) };
        let mut next = self.queues[PLANE].first();
        iter::from_fn(move || {
            let index = next?;
            // SAFETY: this private queue contains only this owner's initialized nodes.
            let entry = unsafe { self.entries.get_unchecked(index.get()) };
            next = entry.hooks[PLANE].next_index();
            Some((index, &entry.value))
        })
    }

    fn front_entry<const PLANE: usize>(
        &mut self,
    ) -> Option<Entry<'_, T, PLANES, phase::Front<PLANE>>> {
        const { assert!(PLANE < PLANES) };
        let index = self.queues[PLANE].first()?;
        Some(Entry::new(self, index))
    }

    fn clear<const PLANE: usize>(&mut self) {
        while let Some(front) = self.front_entry::<PLANE>() {
            front.pop();
        }
    }

    fn first<'a, P: Ord, C: alloc::raw::Extent>(
        &'a mut self,
        heap: &'a mut indexed::Min<P, C>,
    ) -> Option<(indexed::Occupied<'a, P, C>, Entry<'a, T, PLANES>)> {
        let timer = heap.first_entry()?;
        // SAFETY: this coupled owner retains a payload for every priority slot.
        let index = unsafe { links::Index::new_usize(timer.index()) };
        Some((timer, Entry::new(self, index)))
    }

    fn select<'a, P: Ord, C: alloc::raw::Extent>(
        &'a mut self,
        heap: &'a mut indexed::Min<P, C>,
        index: links::Index,
    ) -> (indexed::Entry<'a, P, C>, Entry<'a, T, PLANES>) {
        let entry = Entry::new(self, index);
        // SAFETY: callers retain both disjoint projections of the same coupled owner.
        let live = unsafe { crate::depot::raw::Proof::new(((heap, entry), index.position())) };
        indexed::Slot::<_, _, _>::from_source(live).entry()
    }
    fn grow_with<P: Ord, C: alloc::raw::Extent>(
        &mut self,
        heap: &mut indexed::Min<P, C>,
        len: C,
        seed: impl planes::Seed<T>,
    ) -> Result<&mut Self, planes::Error<C>> {
        let old = self.entries.len();
        if len.len() <= old {
            return Ok(self);
        }
        let mut next = indexed::Min::try_new(len).map_err(planes::Error::Heap)?;
        let core = self
            .grow(len.len() as u32, seed)
            .map_err(|error| planes::Error::Values {
                capacity: len,
                error,
            })?;
        while let Some(timer) = heap.first_entry() {
            // SAFETY: the coupled owner retains one payload for every old priority.
            let index = unsafe { links::Index::new_usize(timer.index()) };
            let entry = Entry::<T, PLANES>::new(core, index);
            let priority = timer.take();
            // SAFETY: the new heap covers this payload, and each old priority moves once.
            let live =
                unsafe { crate::depot::raw::Proof::new(((&mut next, entry), index.position())) };
            let slot = indexed::Slot::<_, _, _, indexed::Empty>::from_source(live);
            let (vacant, _) = slot.entry();
            vacant.set(priority);
        }
        *heap = next;
        Ok(core)
    }

    /// The admitted prefix is represented and every selected-plane hook is vacant.
    unsafe fn link_prefix<const PLANE: usize>(&mut self, len: usize) {
        const { assert!(PLANE < PLANES) };
        for index in 0..len {
            // SAFETY: the caller retains this exact prefix and its vacant hook phase.
            let index = unsafe { links::Index::new_usize(index) };
            Entry::<_, PLANES, phase::Vacant<PLANE>>::new(self, index).link_back();
        }
    }
}

impl<T, P: Ord, const PLANES: usize, C: alloc::raw::Extent> planes::Priority<T, P, PLANES, C>
    for Set<T, PLANES, indexed::Min<P, C>>
{
    fn try_seed(len: C, seed: impl planes::Seed<T>) -> Result<Self, planes::Error<C>> {
        let index = indexed::Min::try_new(len).map_err(planes::Error::Heap)?;
        let values = planes::Set::<T, PLANES>::try_seed(len.len(), seed).map_err(|error| {
            planes::Error::Values {
                capacity: len,
                error,
            }
        })?;
        Ok(Self {
            core: values.core,
            index,
        })
    }

    fn try_repeat(len: C, priority: P) -> Result<Self, planes::Error<C>>
    where
        T: Default,
        P: Copy,
    {
        let index = indexed::Min::try_repeat(len, priority).map_err(planes::Error::Heap)?;
        let values = planes::Set::<T, PLANES>::try_seed(len.len(), ()).map_err(|error| {
            planes::Error::Values {
                capacity: len,
                error,
            }
        })?;
        Ok(Self {
            core: values.core,
            index,
        })
    }

    /// Removes priorities and lends their payloads, preserving queue membership.
    fn reset<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a,
    {
        self.index.clear();
        self.iter_mut()
    }

    fn heap(&self) -> &indexed::Min<P, C> {
        &self.index
    }

    /// Selects a payload and its exact priority slot with one domain check.
    fn select(
        &mut self,
        index: usize,
    ) -> Option<(indexed::Entry<'_, P, C>, planes::Entry<'_, T, PLANES>)> {
        if index >= self.len() {
            return None;
        }
        // SAFETY: the coupled constructor and growth retain equal domains.
        let index = unsafe { links::Index::new_usize(index) };
        Some(self.core.select(&mut self.index, index))
    }

    /// Selects the minimum priority together with its original payload owner.
    fn first(&mut self) -> Option<(indexed::Occupied<'_, P, C>, planes::Entry<'_, T, PLANES>)> {
        self.core.first(&mut self.index)
    }

    /// Grows both domains atomically, retaining every priority and queue link.
    fn try_grow_to(
        &mut self,
        len: C,
        seed: impl planes::Seed<T>,
    ) -> Result<ops::Range<usize>, planes::Error<C>> {
        let old = self.core.entries.len();
        let core = self.core.grow_with(&mut self.index, len, seed)?;
        Ok(old..core.entries.len())
    }
}

const _: () = {
    assert!(
        mem::size_of::<planes::Set<(), 3, indexed::Min<u64>>>()
            == mem::size_of::<planes::Set<(), 3>>() + mem::size_of::<indexed::Min<u64>>()
    );
    assert!(mem::size_of::<planes::Entry<'static, (), 3>>() == 2 * mem::size_of::<usize>());
};

impl<T, P: Ord, const PLANES: usize, const MAX: usize>
    Set<T, PLANES, indexed::Min<P, capacity::Limit<MAX>>>
{
    /// Creates a fresh domain and links its admitted prefix into one plane.
    pub fn try_seed_in<const PLANE: usize>(
        capacity: capacity::Limit<MAX>,
        seed: impl Seed<T>,
    ) -> Result<Self, Error<capacity::Limit<MAX>>> {
        let mut values = <Self as Priority<T, P, PLANES, _>>::try_seed(capacity, seed)?;
        // SAFETY: construction admits the entire fresh prefix; all its hooks are vacant.
        unsafe { values.core.link_prefix::<PLANE>(capacity.get()) };
        Ok(values)
    }
}

impl<T, P: Ord, R, const PLANES: usize, const MAX: usize, const N: usize, const MULTIPLIER: usize>
    TryFrom<Set<T, PLANES, indexed::Min<P, capacity::Limit<MAX>>>>
    for Set<T, PLANES, groups::Index<P, capacity::Limit<MAX>, R, N, MULTIPLIER>>
{
    type Error = alloc::Error;

    fn try_from(
        source: Set<T, PLANES, indexed::Min<P, capacity::Limit<MAX>>>,
    ) -> Result<Self, Self::Error> {
        let records = groups::Index::<P, _, R, N, MULTIPLIER>::reserve(source.index.capacity())?;
        Ok(Self {
            core: source.core,
            index: groups::Index {
                priority: source.index,
                records,
            },
        })
    }
}

impl<T, P: Ord, R, const PLANES: usize, const MAX: usize, const N: usize, const MULTIPLIER: usize>
    Set<T, PLANES, groups::Index<P, capacity::Limit<MAX>, R, N, MULTIPLIER>>
{
    /// Prepares a new prefix, retaining an exclusive loan until companion commit.
    /// Dropping the commit retains grown storage but preserves the active prefix and memberships.
    pub fn try_replace<const PLANE: usize>(
        &mut self,
        records: runtime::Fifo<
            R,
            groups::Rows<capacity::Limit<MAX>, N>,
            groups::Buckets<capacity::Limit<MAX>, MULTIPLIER>,
            links::List,
        >,
        seed: impl Seed<T>,
    ) -> Result<impl FnOnce() + '_, Error<capacity::Limit<MAX>>> {
        let capacity = records.capacity().0;
        let target = &mut self.index.records;
        let core = self
            .core
            .grow_with(&mut self.index.priority, capacity, seed)?;
        Ok(move || {
            core.clear::<PLANE>();
            // SAFETY: successful growth retains this owner and its represented prefix.
            // Clearing the same owner detached every selected-plane hook.
            unsafe { core.link_prefix::<PLANE>(capacity.get()) };
            *target = records;
        })
    }

    /// Returns the active prefix within the retained physical rows.
    pub fn limit(&self) -> capacity::Limit<MAX> {
        self.index.records.capacity().0
    }

    pub fn heap(&self) -> &indexed::Min<P, capacity::Limit<MAX>> {
        &self.index.priority
    }

    pub fn first(
        &mut self,
    ) -> Option<(
        indexed::Occupied<'_, P, capacity::Limit<MAX>>,
        Entry<'_, T, PLANES>,
    )> {
        self.core.first(&mut self.index.priority)
    }
}
