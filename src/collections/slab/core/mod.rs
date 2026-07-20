use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;
use std::mem::{self, ManuallyDrop};

use super::GenerationState;
use std::hint::unreachable_unchecked;

use crate::collections::ClearGuard;
use crate::marker::ThreadBound;

mod guards;

use guards::{Busy, Initializing};

pub(super) const NONE: u32 = u32::MAX;
const USED: u32 = NONE - 1;

pub(super) trait Mode {
    const REENTRANT: bool;
}

pub(super) struct Exclusive;

impl Mode for Exclusive {
    const REENTRANT: bool = false;
}

pub(super) struct Interior;

impl Mode for Interior {
    const REENTRANT: bool = true;
}

#[derive(Clone, Copy)]
pub(crate) struct SlotIndex(u32);

impl SlotIndex {
    pub(crate) fn new(index: u32, capacity: usize) -> Option<Self> {
        ((index as usize) < capacity).then_some(Self(index))
    }

    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Reserved,
    Occupied,
    Busy,
    Retired,
}

#[derive(Clone, Copy)]
struct Links {
    next: u32,
    prev: u32,
}

union Data<T> {
    links: Links,
    value: ManuallyDrop<T>,
}

struct Slot<T, G: Copy> {
    state: Cell<State>,
    generation: Cell<G>,
    position: Cell<u32>,
    data: UnsafeCell<Data<T>>,
}

impl<T, G: Copy> Slot<T, G> {
    fn free(generation: G, next: u32, prev: u32) -> Self {
        Self {
            state: Cell::new(State::Free),
            generation: Cell::new(generation),
            position: Cell::new(NONE),
            data: UnsafeCell::new(Data {
                links: Links { next, prev },
            }),
        }
    }

    unsafe fn links(&self) -> Links {
        unsafe { (*self.data.get()).links }
    }

    unsafe fn set_links(&self, links: Links) {
        unsafe { (*self.data.get()).links = links };
    }

    unsafe fn value(&self) -> &T {
        unsafe { &*(&raw const (*self.data.get()).value).cast::<T>() }
    }

    unsafe fn value_ptr(&self) -> *mut T {
        unsafe { (&raw mut (*self.data.get()).value).cast::<T>() }
    }

    unsafe fn write_value(&self, value: T) {
        unsafe { (*self.data.get()).value = ManuallyDrop::new(value) };
    }

    unsafe fn take_value(&self) -> T {
        unsafe { ManuallyDrop::take(&mut (*self.data.get()).value) }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Ticket<G> {
    pub(super) index: SlotIndex,
    pub(super) generation: G,
}

pub(super) struct SlabCore<T, G: GenerationState, M: Mode> {
    slots: Box<[Slot<T, G>]>,
    occupied: Box<[Cell<u32>]>,
    free: Cell<u32>,
    len: Cell<u32>,
    _thread: ThreadBound,
    mode: PhantomData<M>,
}

impl<T, G: GenerationState, M: Mode> SlabCore<T, G, M> {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        let () = G::VALID;
        assert!(capacity <= USED as usize, "slab capacity overflow");
        Self {
            slots: (0..capacity)
                .map(|index| {
                    Slot::free(
                        G::MIN,
                        if index + 1 == capacity {
                            NONE
                        } else {
                            index as u32 + 1
                        },
                        if index == 0 { NONE } else { index as u32 - 1 },
                    )
                })
                .collect(),
            occupied: (0..capacity).map(|_| Cell::new(NONE)).collect(),
            free: Cell::new(if capacity == 0 { NONE } else { 0 }),
            len: Cell::new(0),
            _thread: ThreadBound::NEW,
            mode: PhantomData,
        }
    }

    pub(super) fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn grow_to(&mut self, capacity: usize) {
        let old_capacity = self.capacity();
        assert!(capacity >= old_capacity, "slab cannot shrink");
        assert!(capacity <= USED as usize, "slab capacity overflow");
        if capacity == old_capacity {
            return;
        }

        let old_free = self.free.get();
        let mut slots = Vec::with_capacity(capacity);
        let mut occupied = Vec::with_capacity(capacity);
        slots.extend(mem::replace(&mut self.slots, Box::new([])));
        occupied.extend(mem::replace(&mut self.occupied, Box::new([])));
        occupied.resize_with(capacity, || Cell::new(NONE));

        for index in old_capacity..capacity {
            slots.push(Slot::free(
                G::MIN,
                if index + 1 == capacity {
                    old_free
                } else {
                    index as u32 + 1
                },
                if index == old_capacity {
                    NONE
                } else {
                    index as u32 - 1
                },
            ));
        }

        self.slots = slots.into_boxed_slice();
        self.occupied = occupied.into_boxed_slice();
        if old_free != NONE {
            self.set_free_prev(old_free, capacity as u32 - 1);
        }
        self.free.set(old_capacity as u32);
    }

    pub(super) fn len(&self) -> usize {
        self.len.get() as usize
    }

    pub(super) fn is_full(&self) -> bool {
        self.free.get() == NONE
    }

    pub(super) fn take_free(&self) -> Option<Ticket<G>> {
        let raw = self.free.get();
        if raw == NONE {
            return None;
        }
        Some(unsafe { self.take_free_raw(raw) })
    }

    unsafe fn take_free_raw(&self, raw: u32) -> Ticket<G> {
        let index = SlotIndex(raw);
        let slot = unsafe { self.slots.get_unchecked(index.get() as usize) };
        if slot.state.get() != State::Free {
            unsafe { unreachable_unchecked() }
        }
        let generation = slot.generation.get();
        let Links { next, prev } = unsafe { slot.links() };
        debug_assert_eq!(prev, NONE);
        if next != NONE {
            let next = unsafe { self.slots.get_unchecked(next as usize) };
            if next.state.get() != State::Free {
                unsafe { unreachable_unchecked() }
            }
            let Links { next: link, .. } = unsafe { next.links() };
            unsafe {
                next.set_links(Links {
                    next: link,
                    prev: NONE,
                })
            };
        }
        self.free.set(next);
        slot.state.set(State::Reserved);
        Ticket { index, generation }
    }

    pub(super) fn take_index(&self, index: u32) -> Option<Ticket<G>> {
        let slot = self.slots.get(index as usize)?;
        let index = SlotIndex::new(index, self.slots.len())?;
        if slot.state.get() != State::Free {
            return None;
        }
        let generation = slot.generation.get();
        let Links { next, prev } = unsafe { slot.links() };
        self.unlink(index, prev, next);
        slot.state.set(State::Reserved);
        Some(Ticket { index, generation })
    }

    fn unlink(&self, index: SlotIndex, prev: u32, next: u32) {
        if prev == NONE {
            debug_assert_eq!(self.free.get(), index.get());
            self.free.set(next);
        } else {
            self.set_free_next(prev, next);
        }
        if next != NONE {
            self.set_free_prev(next, prev);
        }
    }

    fn release(&self, index: SlotIndex, generation: G) {
        let head = self.free.replace(index.get());
        let slot = unsafe { self.slots.get_unchecked(index.get() as usize) };
        slot.generation.set(generation);
        unsafe {
            slot.set_links(Links {
                next: head,
                prev: NONE,
            })
        };
        slot.state.set(State::Free);
        if head != NONE {
            self.set_free_prev(head, index.get());
        }
    }

    fn set_free_next(&self, index: u32, next: u32) {
        let slot = self.free_slot(index);
        let Links { prev, .. } = unsafe { slot.links() };
        unsafe { slot.set_links(Links { next, prev }) };
    }

    fn set_free_prev(&self, index: u32, prev: u32) {
        let slot = self.free_slot(index);
        let Links { next, .. } = unsafe { slot.links() };
        unsafe { slot.set_links(Links { next, prev }) };
    }

    fn free_slot(&self, index: u32) -> &Slot<T, G> {
        let slot = unsafe { self.slots.get_unchecked(index as usize) };
        if slot.state.get() != State::Free {
            unsafe { unreachable_unchecked() }
        }
        slot
    }

    pub(super) fn commit(&self, ticket: Ticket<G>, value: T) {
        let slot = self.reserved_slot(ticket);
        unsafe { slot.write_value(value) };
        self.commit_initialized(ticket);
    }

    pub(super) fn commit_with<R>(
        &self,
        ticket: Ticket<G>,
        value: T,
        f: impl FnOnce(&mut T) -> R,
    ) -> R {
        let mut initializing = Initializing::new(self, ticket, value);
        let result = f(initializing.value_mut());
        initializing.commit();
        result
    }

    fn commit_initialized(&self, ticket: Ticket<G>) {
        let slot = self.reserved_slot(ticket);
        let position = self.len.get();
        unsafe { self.occupied.get_unchecked(position as usize) }.set(ticket.index.get());
        slot.position.set(position);
        slot.state.set(State::Occupied);
        self.len.set(position + 1);
    }

    pub(super) fn rollback(&self, ticket: Ticket<G>) {
        let slot = self.reserved_slot(ticket);
        match ticket.generation.next() {
            Some(generation) => self.release(ticket.index, generation),
            None => slot.state.set(State::Retired),
        }
    }

    fn reserved_slot(&self, ticket: Ticket<G>) -> &Slot<T, G> {
        let slot = unsafe { self.slots.get_unchecked(ticket.index.get() as usize) };
        debug_assert!(
            slot.state.get() == State::Reserved && slot.generation.get() == ticket.generation
        );
        slot
    }

    pub(super) fn contains(&self, index: u32, generation: G) -> bool {
        let Some(slot) = self.slots.get(index as usize) else {
            return false;
        };
        slot.state.get() == State::Occupied && slot.generation.get() == generation
    }

    pub(super) fn get(&self, index: u32, generation: G) -> Option<&T> {
        let slot = self.slots.get(index as usize)?;
        if slot.state.get() == State::Occupied && slot.generation.get() == generation {
            Some(unsafe { slot.value() })
        } else {
            None
        }
    }

    pub(super) fn get_mut(&mut self, index: u32, generation: G) -> Option<&mut T> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.state.get() == State::Occupied && slot.generation.get() == generation {
            Some(unsafe { &mut *slot.value_ptr() })
        } else {
            None
        }
    }

    pub(super) fn remove(&self, index: u32, generation: G) -> Option<(T, SlotIndex)> {
        Some(Busy::take_key(self, index, generation)?.commit_removal())
    }

    pub(super) fn remove_with<R>(
        &self,
        index: u32,
        generation: G,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        let mut busy = Busy::take_key(self, index, generation)?;
        let result = f(busy.value_mut())?;
        let (value, _) = busy.commit_removal();
        Some((value, result))
    }

    pub(super) fn remove_index(&self, index: u32) -> Option<(T, SlotIndex)> {
        Some(Busy::take(self, index)?.commit_removal())
    }

    pub(super) fn remove_index_with<R>(
        &self,
        index: u32,
        f: impl FnOnce(&mut T, G) -> Option<R>,
    ) -> Option<(T, R, SlotIndex)> {
        let mut busy = Busy::take(self, index)?;
        let generation = busy.generation();
        let result = f(busy.value_mut(), generation)?;
        let (value, index) = busy.commit_removal();
        Some((value, result, index))
    }

    pub(super) fn get_index(&self, index: u32) -> Option<(&T, G)> {
        let slot = self.slots.get(index as usize)?;
        if slot.state.get() == State::Occupied {
            Some((unsafe { slot.value() }, slot.generation.get()))
        } else {
            None
        }
    }

    pub(super) fn get_index_mut(&mut self, index: u32) -> Option<(&mut T, G)> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.state.get() == State::Occupied {
            let generation = slot.generation.get();
            Some((unsafe { &mut *slot.value_ptr() }, generation))
        } else {
            None
        }
    }

    pub(super) fn generation(&self, index: u32) -> Option<G> {
        let slot = self.slots.get(index as usize)?;
        (slot.state.get() == State::Occupied).then(|| slot.generation.get())
    }

    pub(super) fn occupied_at(&self, position: usize) -> Option<(u32, G)> {
        if position >= self.len() {
            return None;
        }
        let index = self.occupied.get(position)?.get();
        let slot = self.slots.get(index as usize)?;
        (slot.state.get() == State::Occupied && slot.position.get() as usize == position)
            .then(|| (index, slot.generation.get()))
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &T> {
        (0..self.len()).map(|position| {
            let index = unsafe { self.occupied.get_unchecked(position) }.get();
            let slot = unsafe { self.slots.get_unchecked(index as usize) };
            debug_assert!(slot.state.get() == State::Occupied);
            unsafe { slot.value() }
        })
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let len = self.len();
        let slots = self.slots.as_mut_ptr();
        let occupied = &self.occupied;
        (0..len).map(move |position| {
            let index = unsafe { occupied.get_unchecked(position) }.get();
            let slot = unsafe { &mut *slots.add(index as usize) };
            debug_assert!(slot.state.get() == State::Occupied);
            unsafe { &mut *slot.value_ptr() }
        })
    }

    pub(super) fn clear(&mut self) {
        while self.len.get() != 0 {
            let position = self.len.get() - 1;
            let index = unsafe { self.occupied.get_unchecked(position as usize) }.get();
            drop(self.remove_index(index).map(|(value, _)| value));
        }
    }

    pub(super) fn update<R>(
        &self,
        index: u32,
        generation: G,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        let mut busy = Busy::take_key(self, index, generation)?;
        Some(f(busy.value_mut()))
    }

    fn remove_occupied(&self, index: SlotIndex) {
        let slot = unsafe { self.slots.get_unchecked(index.get() as usize) };
        let position = slot.position.replace(NONE);
        let last_position = self.len.get() - 1;
        let last_index = unsafe { self.occupied.get_unchecked(last_position as usize) }.get();
        unsafe { self.occupied.get_unchecked(position as usize) }.set(last_index);
        unsafe { self.occupied.get_unchecked(last_position as usize) }.set(NONE);
        if last_index != index.get() {
            unsafe { self.slots.get_unchecked(last_index as usize) }
                .position
                .set(position);
        }
        self.len.set(last_position);
    }
}

impl<T, G: GenerationState, M: Mode> Drop for SlabCore<T, G, M> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}
