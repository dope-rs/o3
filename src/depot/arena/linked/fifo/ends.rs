//! Explicit unsafe boundary for external FIFO endpoints.

use std::{cell, mem};

use crate::depot::{
    self,
    arena::linked::{fifo, fifo::ends, slots},
};

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(in crate::depot) struct State<L: crate::depot::raw::Link> {
    head: L,
    tail: L,
}

impl<L: crate::depot::raw::Link> State<L> {
    const EMPTY: Self = Self {
        head: L::NONE,
        tail: L::NONE,
    };

    fn is_empty(self) -> bool {
        self.head == L::NONE
    }

    unsafe fn push_front(&mut self, index: L::Item, publish: impl FnOnce(Option<L::Item>)) {
        let previous = option(self.head);
        publish(previous);
        let link = unsafe { L::some(index) };
        if previous.is_none() {
            self.tail = link;
        }
        self.head = link;
    }

    unsafe fn push_back(&mut self, index: L::Item, publish: impl FnOnce(Option<L::Item>)) {
        let previous = option(self.tail);
        publish(previous);
        let link = unsafe { L::some(index) };
        self.set_back(link, previous.is_none());
    }

    fn set_back(&mut self, link: L, empty: bool) {
        if empty {
            self.head = link;
        }
        self.tail = link;
    }

    unsafe fn append(
        &mut self,
        source: Self,
        front: crate::depot::raw::Present<L>,
        publish: impl FnOnce(Option<L::Item>, L::Item),
    ) {
        let previous = option(self.tail);
        publish(previous, front.item());
        if previous.is_none() {
            self.head = source.head;
        }
        self.tail = source.tail;
    }

    pub(in crate::depot) fn pop<T, R>(
        &mut self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R {
        let (next, result) = pop(front, input);
        let empty = next == L::NONE;
        self.set_front(next, empty);
        result
    }

    fn set_front(&mut self, next: L, empty: bool) {
        self.head = next;
        if empty {
            self.tail = L::NONE;
        }
    }
}

/// Move-only ownership of one external forward chain.
#[repr(transparent)]
#[must_use = "dropping an exclusive chain abandons ownership of its external links"]
#[derive(Debug)]
pub struct Exclusive<L: crate::depot::raw::Link = u32>(pub(in crate::depot) State<L>);

/// Interior-mutable FIFO topology over external links.
#[repr(transparent)]
pub struct Cell<L: crate::depot::raw::Link = u32>(pub(in crate::depot) cell::Cell<State<L>>);

/// Owner-bound proof of one present exclusive FIFO front.
#[must_use = "a present front must be consumed or left linked"]
pub struct Front<O, L: crate::depot::raw::Link = u32> {
    owner: O,
    present: crate::depot::raw::Present<L>,
}

impl<O, L: crate::depot::raw::Link> Front<O, L> {
    pub fn get(&self) -> L::Item {
        self.present.item()
    }
}

impl<L: depot::raw::Link, O: crate::depot::raw::EndsPop<L>> Front<O, L> {
    /// Detaches the source-bound front through its proved exact callback.
    pub fn pop<T, R, F: FnOnce(L::Item, T) -> (L, R)>(
        proof: depot::raw::Proof<(Self, T, F), fifo::Detach>,
    ) -> R {
        let (front, input, pop) = proof.into_inner();
        unsafe { front.owner.pop(front.present.item(), input, pop) }
    }
}

impl<L: depot::raw::Link> Front<&Cell<L>, L> {
    /// Detaches the proved current front and retains its exact successor with the same owner.
    pub fn advance<T, R, F: FnOnce(L::Item, T) -> (L, R)>(
        proof: depot::raw::Proof<(Self, T, F), fifo::Detach>,
    ) -> (R, Option<Self>) {
        let (front, input, pop) = proof.into_inner();
        let mut state = front.owner.0.get();
        let result = state.pop(front.present.item(), input, pop);
        front.owner.0.set(state);
        let next = depot::raw::Present::new(state.head).map(|present| Self {
            owner: front.owner,
            present,
        });
        (result, next)
    }
}

impl<L: crate::depot::raw::Link> Exclusive<L> {
    pub const fn empty() -> Self {
        Self(State::EMPTY)
    }

    pub fn head(&self) -> Option<L::Item> {
        option(self.0.head)
    }

    pub(crate) const fn head_raw(&self) -> L {
        self.0.head
    }

    pub(crate) const fn tail_raw(&self) -> L {
        self.0.tail
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn front(&mut self) -> Option<Front<&mut Self, L>> {
        let present = crate::depot::raw::Present::new(self.0.head)?;
        Some(Front {
            owner: self,
            present,
        })
    }

    /// Prepends the retained detached slot through its exact publication callback.
    pub fn push_front<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&mut Self, L::Item, F), fifo::Prepend>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        unsafe { owner.0.push_front(index, publish) };
    }

    /// Appends the retained detached tail through its exact publication callback.
    pub fn push_back<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&mut Self, L::Item, F), fifo::Append>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        unsafe { owner.0.push_back(index, publish) };
    }

    /// Appends the proved disjoint source chain and empties its original owner.
    pub fn append<F: FnOnce(Option<L::Item>, L::Item)>(
        proof: depot::raw::Proof<(&mut Self, &mut Self, F), fifo::Join>,
    ) {
        let (owner, source, publish) = proof.into_inner();
        let chain = source.0;
        let Some(front) = depot::raw::Present::new(chain.head) else {
            return;
        };
        unsafe { owner.0.append(chain, front, publish) };
        source.0 = State::EMPTY;
    }

    pub(crate) unsafe fn detach<T, R>(
        &mut self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R {
        self.0.pop(front, input, pop)
    }
}

impl<L: crate::depot::raw::Link> Cell<L> {
    pub const fn empty() -> Self {
        Self(cell::Cell::new(State::EMPTY))
    }

    /// # Safety
    /// Both ends retain one valid exclusive chain, or both are absent.
    pub(in crate::depot) const unsafe fn from_parts(head: L, tail: L) -> Self {
        Self(cell::Cell::new(State { head, tail }))
    }

    pub fn head(&self) -> Option<L::Item> {
        option(self.0.get().head)
    }

    pub fn back(&self) -> Option<L::Item> {
        option(self.0.get().tail)
    }

    pub fn is_empty(&self) -> bool {
        self.0.get().is_empty()
    }

    pub fn front(&self) -> Option<Front<&Self, L>> {
        let present = crate::depot::raw::Present::new(self.0.get().head)?;
        Some(Front {
            owner: self,
            present,
        })
    }

    /// Detaches the current front through its source-bound callback, if one is present.
    pub fn pop_front<T, R, F: FnOnce(L::Item, T) -> (L, R)>(
        proof: depot::raw::Proof<(&Self, T, F), fifo::Detach>,
    ) -> Option<R> {
        let (owner, input, pop) = proof.into_inner();
        let front = depot::raw::Present::new(owner.0.get().head)?;
        let (next, result) = pop(front.item(), input);
        let empty = next == L::NONE;
        // SAFETY: Detach retains the same endpoints through the completed callback.
        // Callback and Link/Eq evaluation finish before the exclusive field writes.
        unsafe { &mut *owner.0.as_ptr() }.set_front(next, empty);
        Some(result)
    }

    /// Prepends the retained detached slot through its exact publication callback.
    pub fn push_front<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&Self, L::Item, F), fifo::Prepend>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        let mut state = owner.0.get();
        unsafe { state.push_front(index, publish) };
        owner.0.set(state);
    }

    /// Appends the retained detached tail through its exact publication callback.
    pub fn push_back<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&Self, L::Item, F), fifo::Append>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        let previous = option(owner.0.get().tail);
        publish(previous);
        let link = unsafe { L::some(index) };
        // SAFETY: Append retains the same endpoints through the completed callback.
        // Callback and Link evaluation finish before the exclusive field writes.
        let state = unsafe { &mut *owner.0.as_ptr() };
        state.set_back(link, previous.is_none());
    }

    /// Appends the proved disjoint source chain and empties its original owner.
    pub fn append<F: FnOnce(Option<L::Item>, L::Item)>(
        proof: depot::raw::Proof<(&Self, &mut Exclusive<L>, F), fifo::Join>,
    ) {
        let (owner, source, publish) = proof.into_inner();
        let chain = source.0;
        let Some(front) = depot::raw::Present::new(chain.head) else {
            return;
        };
        let mut state = owner.0.get();
        unsafe { state.append(chain, front, publish) };
        source.0 = State::EMPTY;
        owner.0.set(state);
    }
}

fn option<L: crate::depot::raw::Link>(link: L) -> Option<L::Item> {
    crate::depot::raw::Present::new(link).map(|present| present.item())
}

const _: () = {
    assert!(mem::size_of::<State<u16>>() == 2 * mem::size_of::<u16>());
    assert!(mem::size_of::<State<u32>>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Exclusive<u16>>() == 2 * mem::size_of::<u16>());
    assert!(mem::size_of::<Exclusive<u32>>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Cell<u16>>() == 2 * mem::size_of::<u16>());
    assert!(mem::size_of::<Cell<u32>>() == 2 * mem::size_of::<u32>());
    assert!(mem::align_of::<Exclusive<u32>>() == mem::align_of::<State<u32>>());
    assert!(mem::size_of::<Option<Exclusive<u16>>>() == mem::size_of::<Option<State<u16>>>());
    assert!(mem::size_of::<Option<Exclusive<u32>>>() == mem::size_of::<Option<State<u32>>>());
    assert!(mem::size_of::<Front<&'static mut Exclusive>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Front<&'static Cell>>() == 2 * mem::size_of::<usize>());
};

impl<Tag> ends::Exclusive<slots::Index<Tag>> {
    pub(crate) unsafe fn append_store<T>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) {
        unsafe { store.set_next(index, slots::Index::NONE) };
        unsafe { self.append_end(store, index) };
    }

    /// # Safety
    /// This sole detached live slot belongs to the store and has no successor.
    pub(crate) unsafe fn append_end<T>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) {
        unsafe {
            self.0.push_back(index, |previous| {
                if let Some(previous) = previous {
                    store.set_next(previous, index);
                }
            });
        }
    }

    pub(crate) unsafe fn prepend_link<T>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) {
        unsafe {
            self.0.push_front(index, |previous| {
                let next = previous.unwrap_or(slots::Index::NONE);
                store.set_next(index, next);
            });
        }
    }

    pub(crate) unsafe fn append_chain<T>(&mut self, store: &slots::Store<T>, source: &mut Self) {
        unsafe {
            Self::append(depot::raw::Proof::new((self, source, |previous, head| {
                if let Some(previous) = previous {
                    store.set_next(previous, head);
                }
            })));
        }
    }

    pub(crate) unsafe fn detach_store<T>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> slots::Index<Tag> {
        unsafe { self.detach(index, (), |index, ()| (store.next(index), index)) }
    }
}

const _: () = assert!(mem::size_of::<ends::Exclusive<slots::Index>>() == 2 * mem::size_of::<u32>());

/// An exclusive partition borrowed together with its exact slot store.
pub(crate) struct Bound<'a, T, Tag = ()> {
    store: &'a slots::Store<T>,
    links: &'a mut Exclusive<slots::Index<Tag>>,
}

impl<'a, T, Tag> Bound<'a, T, Tag> {
    /// # Safety
    /// `links` owns a disjoint live partition in `store`, with tracked reclamation.
    pub(crate) unsafe fn new(
        store: &'a slots::Store<T>,
        links: &'a mut Exclusive<slots::Index<Tag>>,
    ) -> Self {
        Self { store, links }
    }

    pub(crate) fn front(
        &mut self,
    ) -> Option<slots::Front<'_, T, Tag, Exclusive<slots::Index<Tag>>>> {
        // SAFETY: the binding retains the exact store and exclusive partition.
        unsafe { slots::Front::new(self.store, self.links) }
    }

    pub(crate) fn clear(&mut self) {
        while let Some(front) = self.front() {
            front.discard();
        }
    }
}

/// Two disjoint partitions certified to belong to one store.
pub(crate) struct Pair<'a, T, Tag = ()> {
    store: &'a slots::Store<T>,
    target: &'a mut Exclusive<slots::Index<Tag>>,
    source: &'a mut Exclusive<slots::Index<Tag>>,
}

impl<'a, T, Tag> Pair<'a, T, Tag> {
    /// # Safety
    /// Both exclusive live partitions belong to `store` and are disjoint.
    pub(crate) unsafe fn new(
        store: &'a slots::Store<T>,
        target: &'a mut Exclusive<slots::Index<Tag>>,
        source: &'a mut Exclusive<slots::Index<Tag>>,
    ) -> Self {
        Self {
            store,
            target,
            source,
        }
    }

    pub(crate) fn append(self) {
        // SAFETY: the pair owns the same-store and disjointness proof through mutation.
        unsafe { self.target.append_chain(self.store, self.source) };
    }
}
