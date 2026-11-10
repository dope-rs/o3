//! Explicit unsafe boundary for external FIFO endpoints.

use std::{cell, mem};

use crate::depot::{
    self,
    arena::linked::{fifo, fifo::ends, slots},
    raw::EndsPop,
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
        let previous = self.head.item();
        publish(previous);
        let link = unsafe { L::some(index) };
        if previous.is_none() {
            self.tail = link;
        }
        self.head = link;
    }

    unsafe fn push_back(&mut self, index: L::Item, publish: impl FnOnce(Option<L::Item>)) {
        let previous = self.tail.item();
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
        front: L::Item,
        publish: impl FnOnce(Option<L::Item>, L::Item),
    ) {
        let previous = self.tail.item();
        publish(previous, front);
        if previous.is_none() {
            self.head = source.head;
        }
        self.tail = source.tail;
    }

    fn pop<T, R>(
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
    present: L::Item,
}

impl<O, L: crate::depot::raw::Link> Front<O, L> {
    pub fn get(&self) -> L::Item {
        self.present
    }
}

impl<L: depot::raw::Link, O: crate::depot::raw::EndsPop<L>> Front<O, L> {
    /// Detaches the source-bound front through its proved exact callback.
    pub fn pop<T, R, F: FnOnce(L::Item, T) -> (L, R)>(
        proof: depot::raw::Proof<(Self, T, F), fifo::Detach>,
    ) -> R {
        let (front, input, pop) = proof.into_inner();
        unsafe { front.owner.pop(front.present, input, pop) }
    }
}

impl<L: depot::raw::Link> Front<&Cell<L>, L> {
    /// Detaches the proved current front and retains its exact successor with the same owner.
    pub fn advance<T, R, F: FnOnce(L::Item, T) -> (L, R)>(
        proof: depot::raw::Proof<(Self, T, F), fifo::Detach>,
    ) -> (R, Option<Self>) {
        let (front, input, pop) = proof.into_inner();
        let mut state = front.owner.0.get();
        let result = state.pop(front.present, input, pop);
        front.owner.0.set(state);
        let next = state.head.item().map(|present| Self {
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
        self.0.head.item()
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
        let present = self.0.head.item()?;
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
        let Some(front) = chain.head.item() else {
            return;
        };
        unsafe { owner.0.append(chain, front, publish) };
        source.0 = State::EMPTY;
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
        self.0.get().head.item()
    }

    pub fn back(&self) -> Option<L::Item> {
        self.0.get().tail.item()
    }

    pub fn is_empty(&self) -> bool {
        self.0.get().is_empty()
    }

    pub fn front(&self) -> Option<Front<&Self, L>> {
        let present = self.0.get().head.item()?;
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
        let front = owner.0.get().head.item()?;
        let (next, result) = pop(front, input);
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
        let previous = owner.0.get().tail.item();
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
        let Some(front) = chain.head.item() else {
            return;
        };
        let mut state = owner.0.get();
        unsafe { state.append(chain, front, publish) };
        source.0 = State::EMPTY;
        owner.0.set(state);
    }
}

unsafe impl<Tag> crate::depot::raw::SlotOwner<Tag> for ends::Exclusive<slots::Index<Tag>> {
    fn links(&self) -> &Self {
        self
    }

    unsafe fn detach<'a, T>(
        &mut self,
        store: &'a slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, &'a slots::Store<T>, Tag> {
        let index = unsafe { self.detach_store(store, index) };
        unsafe { store.detached(index) }
    }
}

unsafe impl<L: depot::raw::Link> EndsPop<L> for &mut Exclusive<L> {
    unsafe fn pop<T, R>(
        self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R {
        self.0.pop(front, input, pop)
    }
}

unsafe impl<L: depot::raw::Link> EndsPop<L> for &Cell<L> {
    unsafe fn pop<T, R>(
        self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R {
        let mut state = self.0.get();
        let result = state.pop(front, input, pop);
        self.0.set(state);
        result
    }
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

    /// # Safety
    /// Both exclusive live partitions belong to `store` and are disjoint.
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
        unsafe { self.pop(index, (), |index, ()| (store.next(index), index)) }
    }

    /// # Safety
    /// This owner retains a disjoint live partition in `store`, with tracked reclamation.
    pub(crate) unsafe fn clear<T>(&mut self, store: &slots::Store<T>) {
        // SAFETY: this borrow retains the exact store and exclusive partition.
        while let Some(front) = unsafe { slots::Front::new(store, self) } {
            front.discard();
        }
    }
}

const _: () = assert!(mem::size_of::<ends::Exclusive<slots::Index>>() == 2 * mem::size_of::<u32>());
