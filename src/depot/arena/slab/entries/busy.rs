use std::{marker, mem, num};

use cycle::Policy;

use crate::depot::{
    arena::slab::{
        self, entries,
        key::{self, cycle},
        storage::{self, Plane},
    },
    index::Initialized,
    queue::links,
};

pub struct Busy<
    'a,
    T,
    const MAX: u32 = { u32::MAX },
    M: entries::Mode = entries::Exclusive<false>,
    const PARTITIONS: usize = 1,
    P = (),
    S = storage::Heap,
    I = storage::Eager,
> where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) plane: marker::PhantomData<fn(P) -> P>,
    pub(in crate::depot) core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
    pub(in crate::depot) slot: &'a crate::depot::raw::SlabSlot<
        T,
        key::Generation<MAX>,
        M::Busy,
        <M::Cycle as Policy>::Retired,
    >,
}

impl<'a, T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, P, S, I>
    Busy<'a, T, MAX, M, PARTITIONS, P, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn index(&self) -> u32 {
        // SAFETY: both constructors retain a slot borrowed from this core's allocation.
        unsafe { self.slot.index(self.core.slots.as_ref()) }.raw()
    }

    pub(in crate::depot) fn generation(&self) -> key::Generation<MAX> {
        self.slot.generation.get()
    }

    pub(in crate::depot) fn value(&self) -> &T {
        unsafe { self.slot.value() }
    }

    pub(in crate::depot) fn value_mut(&mut self) -> &mut T {
        unsafe { &mut *self.slot.value_ptr() }
    }

    fn retire(self) {
        let busy = mem::ManuallyDrop::new(self);
        busy.detach();
        entries::Pending {
            plane: busy.plane,
            core: busy.core,
            slot: busy.slot,
        }
        .reject();
    }

    fn detach(&self) {
        let pos = self.slot.position.get();
        // SAFETY: Busy retains an initialized member of this exact dense prefix.
        let len = unsafe { num::NonZero::<u32>::new_unchecked(self.core.len.get()) };
        let tail = len.get() - 1;
        let view = &self.core.slots;
        if pos != tail {
            // SAFETY: distinct positions retain distinct live slots in this initialized
            // prefix. The removed suffix is unread until insertion overwrites it.
            unsafe {
                let last = view.tail().get_unchecked(tail as usize).get().assume_init();
                view.tail()
                    .get_unchecked(pos as usize)
                    .set(mem::MaybeUninit::new(last));
                view.as_ref().get_unchecked(last as usize).position.set(pos);
            }
        }
        self.core.len.set(tail);
    }

    pub(in crate::depot) fn remove(self) -> T {
        let value = unsafe { self.slot.take_value() };
        self.retire();
        value
    }

    pub(in crate::depot) fn discard(self) {
        unsafe { self.slot.drop_value() };
        self.retire();
    }
}

impl<'a, T, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, P, S, I>
    Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn withdraw(
        self,
    ) -> (
        entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS, P, S, I>,
        T,
    ) {
        // SAFETY: this Exclusive guard owns the initialized slot's sole take authority.
        // Its loan passes to Pending; Exclusive's Drop does not access the slot.
        let value = unsafe { self.slot.take_value() };
        self.detach();
        self.slot.state.set(slab::State::Reserved);
        (
            entries::Pending {
                plane: self.plane,
                core: self.core,
                slot: self.slot,
            },
            value,
        )
    }

    pub(in crate::depot) fn read<Q>(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<Q>>,
    ) -> &T {
        Self::project(live).1
    }

    pub(in crate::depot) fn project<Q>(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<Q>>,
    ) -> (key::Generation<MAX>, &T) {
        let slots = live.source().core.slots.as_ref();
        // SAFETY: Live retains an initialized slot in this exact core and a shared
        // reborrow of the root guard's exclusive loan through the returned reference.
        let slot = unsafe { slots.get_unchecked(live.key().get()) };
        (slot.generation.get(), unsafe { slot.value() })
    }

    pub(in crate::depot) fn into_mut(self) -> &'a mut T {
        // SAFETY: consuming this Exclusive guard transfers its original exclusive
        // loan of the initialized slot; Exclusive's Drop does not access the slot.
        unsafe { &mut *self.slot.value_ptr() }
    }

    pub(in crate::depot) fn replace(mut self, next: key::Generation<MAX>, value: T) -> (Self, T) {
        let prior = mem::replace(self.value_mut(), value);
        self.slot.generation.set(next);
        (self, prior)
    }
}

impl<T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, P, S, I> Drop
    for Busy<'_, T, MAX, M, PARTITIONS, P, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    fn drop(&mut self) {
        if M::REENTRANT {
            self.slot.state.set(slab::State::Occupied);
        }
    }
}

impl<T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, P, S, I> crate::mem::Return
    for Busy<'_, T, MAX, M, PARTITIONS, P, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    type Item = ();

    fn return_item(self, (): ()) {
        self.discard();
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool>
    slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub(in crate::depot) fn new(
        entry: entries::Occupied<'a, T, MAX, entries::Interior<RECYCLE>, 1>,
    ) -> Self {
        Self {
            entry: entry.take(),
            tag: marker::PhantomData,
        }
    }

    pub fn key(&self) -> key::Handle<Tag, MAX> {
        key::Handle::new(self.entry.index(), self.entry.generation())
    }

    pub fn value(&self) -> &T {
        self.entry.value()
    }

    pub fn value_mut(&mut self) -> &mut T {
        self.entry.value_mut()
    }

    pub fn remove(self) -> T {
        self.entry.remove()
    }

    pub fn discard(self) {
        self.entry.discard();
    }
}
