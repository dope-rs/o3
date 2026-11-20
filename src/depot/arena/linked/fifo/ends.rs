//! Explicit unsafe boundary for external FIFO endpoints.

use std::{cell, mem};

use crate::depot::{
    self,
    arena::linked::{self, fifo, fifo::ends, slots},
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

    unsafe fn push_front(
        ends: &cell::Cell<Self>,
        index: L::Item,
        publish: impl FnOnce(Option<L::Item>),
    ) {
        let previous = ends.get().head.item();
        publish(previous);
        let link = unsafe { L::some(index) };
        Self::set_front(ends, link, link, previous.is_none());
    }

    unsafe fn push_back(
        ends: &cell::Cell<Self>,
        index: L::Item,
        publish: impl FnOnce(Option<L::Item>),
    ) {
        let previous = ends.get().tail.item();
        publish(previous);
        let link = unsafe { L::some(index) };
        Self::set_back(ends, link, link, previous.is_none());
    }

    fn set_back(ends: &cell::Cell<Self>, head: L, tail: L, empty: bool) {
        // SAFETY: callbacks and Link methods have returned. Only Copy field writes follow.
        let state = unsafe { &mut *ends.as_ptr() };
        if empty {
            state.head = head;
        }
        state.tail = tail;
    }

    unsafe fn append(
        ends: &cell::Cell<Self>,
        source: &mut Self,
        publish: impl FnOnce(Option<L::Item>, L::Item),
    ) {
        let Some(front) = source.head.item() else {
            return;
        };
        let previous = ends.get().tail.item();
        publish(previous, front);
        Self::set_back(ends, source.head, source.tail, previous.is_none());
        *source = Self::EMPTY;
    }

    fn set_front(ends: &cell::Cell<Self>, head: L, tail: L, empty: bool) {
        // SAFETY: no callback or Link method runs during these Copy field writes.
        let state = unsafe { &mut *ends.as_ptr() };
        state.head = head;
        if empty {
            state.tail = tail;
        }
    }

    fn advance(ends: &cell::Cell<Self>, next: L) {
        Self::set_front(ends, next, L::NONE, next == L::NONE);
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
        let (next, result) = pop(front.present, input);
        unsafe { front.owner.advance(next) };
        result
    }
}

impl<L: depot::raw::Link> Front<&Cell<L>, L> {
    /// Detaches the proved current front and retains its exact successor with the same owner.
    pub fn advance<T, R, F: FnOnce(L::Item, T) -> (L, R)>(
        proof: depot::raw::Proof<(Self, T, F), fifo::Detach>,
    ) -> (R, Option<Self>) {
        let (front, input, pop) = proof.into_inner();
        let (next, result) = pop(front.present, input);
        unsafe { front.owner.advance(next) };
        let next = next.item().map(|present| Self {
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
        unsafe { State::push_front(cell::Cell::from_mut(&mut owner.0), index, publish) };
    }

    /// Appends the retained detached tail through its exact publication callback.
    pub fn push_back<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&mut Self, L::Item, F), fifo::Append>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        unsafe { State::push_back(cell::Cell::from_mut(&mut owner.0), index, publish) };
    }

    /// Appends the proved disjoint source chain and empties its original owner.
    pub fn append<F: FnOnce(Option<L::Item>, L::Item)>(
        proof: depot::raw::Proof<(&mut Self, &mut Self, F), fifo::Join>,
    ) {
        let (owner, source, publish) = proof.into_inner();
        unsafe { State::append(cell::Cell::from_mut(&mut owner.0), &mut source.0, publish) };
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
        let front = owner.head()?;
        let (next, result) = pop(front, input);
        unsafe { owner.advance(next) };
        Some(result)
    }

    /// Prepends the retained detached slot through its exact publication callback.
    pub fn push_front<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&Self, L::Item, F), fifo::Prepend>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        unsafe { State::push_front(&owner.0, index, publish) };
    }

    /// Appends the retained detached tail through its exact publication callback.
    pub fn push_back<F: FnOnce(Option<L::Item>)>(
        proof: depot::raw::Proof<(&Self, L::Item, F), fifo::Append>,
    ) {
        let (owner, index, publish) = proof.into_inner();
        unsafe { State::push_back(&owner.0, index, publish) };
    }

    /// Appends the proved disjoint source chain and empties its original owner.
    pub fn append<F: FnOnce(Option<L::Item>, L::Item)>(
        proof: depot::raw::Proof<(&Self, &mut Exclusive<L>, F), fifo::Join>,
    ) {
        let (owner, source, publish) = proof.into_inner();
        unsafe { State::append(&owner.0, &mut source.0, publish) };
    }
}

unsafe impl<L: depot::raw::Link> EndsPop<L> for &mut Exclusive<L> {
    unsafe fn advance(self, next: L) {
        State::advance(cell::Cell::from_mut(&mut self.0), next);
    }
}

unsafe impl<L: depot::raw::Link> EndsPop<L> for &Cell<L> {
    unsafe fn advance(self, next: L) {
        State::advance(&self.0, next);
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
    /// # Safety
    /// Both ends delimit the same exclusively owned, nonempty partition.
    pub(crate) const unsafe fn from_ends(head: slots::Index<Tag>, tail: slots::Index<Tag>) -> Self {
        Self(State { head, tail })
    }

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
            State::push_back(cell::Cell::from_mut(&mut self.0), index, |previous| {
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
            State::push_front(cell::Cell::from_mut(&mut self.0), index, |previous| {
                let next = previous.unwrap_or(slots::Index::NONE);
                store.set_next(index, next);
            });
        }
    }

    /// # Safety
    /// Both partitions belong to `store` and are disjoint. `source` is nonempty.
    pub(crate) unsafe fn append_chain<T>(&mut self, store: &slots::Store<T>, source: Self) {
        if self.0.tail == slots::Index::NONE {
            self.0.head = source.0.head;
        } else {
            unsafe { store.set_next(self.0.tail, source.0.head) };
        }
        self.0.tail = source.0.tail;
    }

    pub(crate) unsafe fn detach_store<T>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> slots::Index<Tag> {
        unsafe { self.advance(store.next(index)) };
        index
    }

    /// # Safety
    /// `index` is this partition's front in the owner's exact store and accounting domain.
    pub(crate) unsafe fn detach<T, H: linked::Source<T>>(
        &mut self,
        owner: H,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, H, Tag> {
        let index = unsafe { self.detach_store(owner.store(), index) };
        unsafe { slots::Live::detached(owner, index) }
    }

    /// # Safety
    /// `index` is this partition's front in `store` with its original TRACK policy.
    pub(crate) unsafe fn pop<T, const TRACK: bool>(
        &mut self,
        store: &slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> T {
        let index = unsafe { self.detach_store(store, index) };
        unsafe { store.take::<_, TRACK>(index) }
    }

    /// # Safety
    /// This owner retains a disjoint live partition in `store`, with tracked reclamation.
    pub(crate) unsafe fn clear<T>(self, store: &slots::Store<T>, mut before: impl FnMut(&T)) {
        let mut head = self.0.head;
        while head != slots::Index::NONE {
            let index = head;
            // SAFETY: consuming the partition transfers its suffix before this slot is reclaimed.
            head = unsafe { store.next(index) };
            let item = unsafe { store.detached(index) };
            before(item.get());
        }
    }
}

const _: () = assert!(mem::size_of::<ends::Exclusive<slots::Index>>() == 2 * mem::size_of::<u32>());
