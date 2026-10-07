//! Explicit unsafe boundary for compact external linked states.

use std::{marker, mem};

pub use cursor::Cursor;
pub use entry::Entry;
pub use index::Index;
pub use live::Live;
pub use reserve::Reserve;
pub use store::Store;
pub(crate) use store::{Capacity, Input, Node};
pub use vacant::Vacant;

use crate::depot::{
    arena::{
        linked,
        linked::{fifo::ends, slots},
    },
    capacity,
};

pub mod fixed;
mod index;
mod live;
mod store;
mod vacant;

mod cursor;
mod entry;
mod reserve;

/// Exclusive proof that one exact node set has insertion capacity.
#[must_use]
pub struct Permit<'a, T, Tag> {
    pub(in crate::depot::arena::linked) store: &'a mut slots::Store<T>,
    domain: marker::PhantomData<fn(Tag) -> Tag>,
}

impl<'a, T, Tag> Permit<'a, T, Tag> {
    pub(super) const fn new(store: &'a mut slots::Store<T>) -> Self {
        Self {
            store,
            domain: marker::PhantomData,
        }
    }

    /// Initializes the slot proved available by this permit.
    pub fn claim(self, value: T) -> Live<T, &'a Store<T>, Tag> {
        unsafe { slots::Vacant::insert_one(&*self.store, value) }
    }
}

const _: () = {
    assert!(mem::size_of::<Permit<'static, u64, ()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Permit<'static, u64, ()>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Vacant<u64, &'static Store<u64>, ()>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Live<u64, &'static Store<u64>, ()>>() == 2 * mem::size_of::<usize>());
};

/// A store borrow whose accounting is fixed by its exclusive topology owner.
pub(crate) struct Borrow<'a, T, const TRACK: bool>(pub(crate) &'a Store<T>);

// SAFETY: topology owners keep this fixed policy from insertion through removal.
impl<T, const TRACK: bool> linked::Sealed<T> for Borrow<'_, T, TRACK> {}

impl<T, const TRACK: bool> linked::Source<T> for Borrow<'_, T, TRACK> {
    const TRACK: bool = TRACK;

    fn store(&self) -> &Store<T> {
        self.0
    }
}

/// Counted FIFO endpoints.
#[repr(C)]
pub struct State<Tag = ()> {
    pub(in crate::depot) links: ends::Exclusive<slots::Index<Tag>>,
    len: u32,
}

impl<Tag> State<Tag> {
    pub(crate) const fn new() -> Self {
        Self {
            links: ends::Exclusive::empty(),
            len: 0,
        }
    }

    pub(crate) const fn count(&self) -> capacity::Count {
        capacity::Count::new(self.len)
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn front_index(&self) -> Option<slots::Index<Tag>> {
        self.links.head()
    }

    pub(crate) fn back_index(&self) -> Option<u32> {
        self.links.tail_raw().get()
    }

    /// # Safety
    /// Reachable indices are this store's unique, acyclic live partition.
    pub(crate) unsafe fn iter<'a, T>(
        &'a self,
        store: &'a slots::Store<T>,
    ) -> linked::Iter<'a, T, Tag> {
        // SAFETY: the caller proves this immutable state's exact live partition.
        unsafe { linked::Iter::new(store, self.links.head_raw()) }
    }

    /// # Safety
    /// Reachable indices are this store's unique live partition.
    pub(crate) unsafe fn front<'a, T>(&'a self, store: &'a slots::Store<T>) -> Option<&'a T> {
        let index = self.links.head()?;
        Some(unsafe { store.value(index) })
    }

    /// Detaches the front value named by its checked owner-bound index.
    /// # Safety
    /// `index` is this state's front and its reachable indices are this store's unique live partition.
    pub(crate) unsafe fn take<T, H: linked::Source<T>>(
        &mut self,
        owner: H,
        index: slots::Index<Tag>,
    ) -> (T, slots::Vacant<T, H, Tag>) {
        unsafe { self.detach(owner, index) }.take()
    }

    /// # Safety
    /// This is the exact store's owned front with its unchanged TRACK policy.
    pub(crate) unsafe fn pop<T, const TRACK: bool>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> T {
        let index = unsafe { self.links.detach_store(store, index) };
        self.len = self.len.wrapping_sub(1);
        unsafe { store.take::<_, TRACK>(index) }
    }

    /// # Safety
    /// The claim belongs to this state's exact store and has no successor.
    pub(crate) unsafe fn append<T, H: linked::Source<T>>(
        &mut self,
        mut claim: slots::Live<T, H, Tag>,
    ) -> H {
        let len = self.len.wrapping_add(1);
        let index = claim.index();
        unsafe { self.links.append_end(claim.owner_mut().store(), index) };
        self.len = len;
        unsafe { claim.publish() }
    }

    pub(in crate::depot) unsafe fn detach<T, H: linked::Source<T>>(
        &mut self,
        owner: H,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, H, Tag> {
        let index = unsafe { self.links.detach_store(owner.store(), index) };
        self.len = self.len.wrapping_sub(1);
        unsafe { slots::Live::detached(owner, index) }
    }
}

const _: () = assert!(mem::size_of::<State>() == 3 * mem::size_of::<u32>());

pub struct Bound<'a, T, Tag = (), const TRACK: bool = true> {
    store: &'a slots::Store<T>,
    state: &'a mut State<Tag>,
}

impl<Tag> State<Tag> {
    /// This state's live partition belongs to this exact store with the
    /// unchanged TRACK accounting policy.
    pub(crate) unsafe fn bind<'a, T, const TRACK: bool>(
        &'a mut self,
        store: &'a slots::Store<T>,
    ) -> Bound<'a, T, Tag, TRACK> {
        Bound { store, state: self }
    }
}

impl<T, Tag, const TRACK: bool> linked::Sealed<T> for Bound<'_, T, Tag, TRACK> {}

impl<T, Tag, const TRACK: bool> linked::Source<T> for Bound<'_, T, Tag, TRACK> {
    const TRACK: bool = TRACK;

    fn store(&self) -> &slots::Store<T> {
        self.store
    }
}

impl<'a, T, Tag> Bound<'a, T, Tag> {
    pub fn pop_front(&mut self) -> Option<T> {
        // SAFETY: this binding retains the exact store and exclusive live partition.
        Some(unsafe { slots::Front::new(self.store, self.state) }?.pop())
    }

    pub fn front_entry(self) -> Option<slots::Front<'a, T, Tag>> {
        unsafe { slots::Front::new(self.store, self.state) }
    }
}

impl<'a, T, Tag, const TRACK: bool> slots::Vacant<T, Bound<'a, T, Tag, TRACK>, Tag> {
    pub fn append(self, value: T) {
        let mut claim = self.insert(value);
        let index = claim.index();
        let owner = unsafe { claim.owner_mut() };
        let len = owner.state.len.wrapping_add(1);
        // SAFETY: insert initialized this exact detached slot with no successor.
        unsafe { owner.state.links.append_end(owner.store, index) };
        let owner = unsafe { claim.publish() };
        owner.state.len = len;
    }
}

impl<'a, T, Tag, const TRACK: bool> slots::Live<T, Bound<'a, T, Tag, TRACK>, Tag> {
    pub(crate) fn prepend(mut self) {
        let index = self.index();
        let owner = unsafe { self.owner_mut() };
        let len = owner.state.len.wrapping_add(1);
        unsafe { owner.state.links.prepend_link(owner.store, index) };
        let owner = unsafe { self.publish() };
        owner.state.len = len;
    }
}

pub struct Front<'a, T, Tag, O = slots::State<Tag>> {
    store: &'a slots::Store<T>,
    owner: &'a mut O,
    index: slots::Index<Tag>,
}

impl<'a, T, Tag, O: crate::depot::raw::SlotOwner<Tag>> Front<'a, T, Tag, O> {
    /// The owner's live partition belongs to this exact store.
    pub(crate) unsafe fn new(store: &'a slots::Store<T>, owner: &'a mut O) -> Option<Self> {
        let index = owner.links().head()?;
        Some(Self {
            store,
            owner,
            index,
        })
    }

    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: this entry's exclusive owner borrow is the capability for
        // the unchanged front slot; the returned borrow cannot outlive `self`.
        unsafe { &mut *self.store.value_ptr(self.index) }
    }

    pub(crate) fn pop(self) -> T {
        unsafe { self.owner.detach(self.store, self.index) }
            .take()
            .0
    }

    pub fn discard(self) {
        unsafe { self.owner.detach(self.store, self.index) }
            .clear()
            .release();
    }
}

const _: () = assert!(mem::size_of::<Front<'static, u64, ()>>() == 3 * mem::size_of::<usize>());
