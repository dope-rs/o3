use std::{
    cell::{Cell, UnsafeCell},
    hint::unreachable_unchecked,
    marker::PhantomData,
    mem::ManuallyDrop,
};

use crate::{
    ThreadBound,
    collections::{
        BoxSliceGrowth, ClearGuard,
        slab::{GenerationState, SlabCapacity},
    },
};

pub(super) mod entries;
mod guards;

use entries::Entries;

pub(super) const NONE: u32 = u32::MAX;

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
pub(super) struct SlotIndex(u32);

impl SlotIndex {
    fn new(index: u32, capacity: usize) -> Option<Self> {
        ((index as usize) < capacity).then_some(Self(index))
    }

    pub(super) fn get(self) -> u32 {
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

pub(super) trait Reservations<T, G: GenerationState, M: Mode> {
    fn take_free(&self) -> Option<Ticket<G>>;
    unsafe fn take_free_raw(&self, raw: u32) -> Ticket<G>;
    fn take_index(&self, index: u32) -> Option<Ticket<G>>;
    fn unlink(&self, index: SlotIndex, prev: u32, next: u32);
    fn release(&self, index: SlotIndex, generation: G);
    fn set_free_next(&self, index: u32, next: u32);
    fn set_free_prev(&self, index: u32, prev: u32);
    fn commit(&self, ticket: Ticket<G>, value: T);
    fn commit_initialized(&self, ticket: Ticket<G>);
    fn rollback(&self, ticket: Ticket<G>);
}

impl<T, G: GenerationState, M: Mode> SlabCore<T, G, M> {
    pub(super) fn with_capacity(capacity: SlabCapacity) -> Self {
        let () = G::VALID;
        let raw_capacity = capacity.get();
        let slots = capacity.collect_box((0..raw_capacity).map(|index| {
            Slot::free(
                G::MIN,
                if index + 1 == raw_capacity {
                    NONE
                } else {
                    index as u32 + 1
                },
                if index == 0 { NONE } else { index as u32 - 1 },
            )
        }));
        let occupied = capacity.collect_box((0..raw_capacity).map(|_| Cell::new(NONE)));
        Self {
            slots,
            occupied,
            free: Cell::new(if raw_capacity == 0 { NONE } else { 0 }),
            len: Cell::new(0),
            _thread: ThreadBound::NEW,
            mode: PhantomData,
        }
    }

    pub(super) fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn grow_to(&mut self, capacity: SlabCapacity) {
        let old_capacity = self.capacity();
        let capacity = capacity.get();
        assert!(
            capacity >= old_capacity,
            "cannot shrink slab from {old_capacity} slots to {capacity} slots"
        );
        if capacity == old_capacity {
            return;
        }

        let old_free = self.free.get();
        let mut slots = BoxSliceGrowth::take(&mut self.slots);
        let mut occupied = BoxSliceGrowth::take(&mut self.occupied);
        let additional = capacity - old_capacity;
        slots.reserve_exact(additional);
        occupied.reserve_exact(additional);
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

        drop(slots);
        drop(occupied);
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
}

impl<T, G: GenerationState, M: Mode> Reservations<T, G, M> for SlabCore<T, G, M> {
    fn take_free(&self) -> Option<Ticket<G>> {
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

    fn take_index(&self, index: u32) -> Option<Ticket<G>> {
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
        let slot = free_slot(self, index);
        let Links { prev, .. } = unsafe { slot.links() };
        unsafe { slot.set_links(Links { next, prev }) };
    }

    fn set_free_prev(&self, index: u32, prev: u32) {
        let slot = free_slot(self, index);
        let Links { next, .. } = unsafe { slot.links() };
        unsafe { slot.set_links(Links { next, prev }) };
    }

    fn commit(&self, ticket: Ticket<G>, value: T) {
        let slot = reserved_slot(self, ticket);
        unsafe { slot.write_value(value) };
        self.commit_initialized(ticket);
    }

    fn commit_initialized(&self, ticket: Ticket<G>) {
        let slot = reserved_slot(self, ticket);
        let position = self.len.get();
        unsafe { self.occupied.get_unchecked(position as usize) }.set(ticket.index.get());
        slot.position.set(position);
        slot.state.set(State::Occupied);
        self.len.set(position + 1);
    }

    fn rollback(&self, ticket: Ticket<G>) {
        let slot = reserved_slot(self, ticket);
        match ticket.generation.next() {
            Some(generation) => self.release(ticket.index, generation),
            None => slot.state.set(State::Retired),
        }
    }
}

fn free_slot<T, G: GenerationState, M: Mode>(core: &SlabCore<T, G, M>, index: u32) -> &Slot<T, G> {
    let slot = unsafe { core.slots.get_unchecked(index as usize) };
    if slot.state.get() != State::Free {
        unsafe { unreachable_unchecked() }
    }
    slot
}

fn reserved_slot<T, G: GenerationState, M: Mode>(
    core: &SlabCore<T, G, M>,
    ticket: Ticket<G>,
) -> &Slot<T, G> {
    let slot = unsafe { core.slots.get_unchecked(ticket.index.get() as usize) };
    debug_assert!(
        slot.state.get() == State::Reserved && slot.generation.get() == ticket.generation
    );
    slot
}

impl<T, G: GenerationState, M: Mode> Drop for SlabCore<T, G, M> {
    fn drop(&mut self) {
        ClearGuard::run(self, Self::clear);
    }
}
