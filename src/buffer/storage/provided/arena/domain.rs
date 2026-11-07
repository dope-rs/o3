use std::{cell, marker, num};

use crate::buffer::storage::{
    provided,
    provided::arena::{self, charge::Counter},
};

/// Intrusive storage whose slot type is fixed by its original arena.
#[repr(transparent)]
pub(in crate::buffer::storage::provided) struct Domain<'d, T, L> {
    pub(super) state: State<'d>,
    slots: SlotDomain<'d, T, L>,
}

impl<'d, T, L> Domain<'d, T, L> {
    pub(super) fn new(capacity: num::NonZero<u32>, limit: u32) -> Self {
        Self {
            state: State::new(capacity, limit.min(capacity.get())),
            slots: marker::PhantomData,
        }
    }
    pub(in crate::buffer::storage::provided) fn next_free<C: Copy>(
        &self,
        context: C,
    ) -> Option<provided::Vacant<'d, T, L, C>> {
        let mut node = self.state.free.take()?;
        self.state.free.set(node.take_next());
        Some(provided::Vacant {
            node: crate::mem::Lease::open(node, ()),
            domain: marker::PhantomData,
            context,
        })
    }
    pub(in crate::buffer::storage::provided) fn pop_returned(
        &self,
    ) -> Option<arena::Returned<'d, T, L>> {
        let (mut node, tail) = self.state.returned.take()?;
        self.state
            .returned
            .set(node.take_next().map(|head| (head, tail)));
        Some(arena::Returned {
            node,
            domain: marker::PhantomData,
        })
    }
}

const _: () = assert!(size_of::<Domain<'_, (), ()>>() == 5 * size_of::<usize>());

/// Intrusive roots and accounting owned by one active arena scope.
#[repr(C)]
pub(in crate::buffer::storage::provided) struct State<'d> {
    free: cell::Cell<Option<arena::Empty<'d>>>,
    returned: cell::Cell<Queue<'d>>,
    pub(super) accounted: Counter,
    capacity: num::NonZero<u32>,
    pub(super) limit: u32,
    brand: marker::PhantomData<fn(&'d ()) -> &'d ()>,
}

impl<'d> State<'d> {
    fn new(capacity: num::NonZero<u32>, limit: u32) -> Self {
        Self {
            free: cell::Cell::new(None),
            returned: cell::Cell::new(None),
            accounted: Counter::default(),
            capacity,
            limit,
            brand: marker::PhantomData,
        }
    }
    pub(in crate::buffer::storage::provided) const fn capacity(&self) -> num::NonZero<u32> {
        self.capacity
    }
    pub(super) fn push_free(&self, node: arena::Empty<'d>) {
        let next = self.free.take();
        // SAFETY: this affine vacancy retains its exact detached slot.
        unsafe { node.entry.as_ref() }.free.set(next);
        self.free.set(Some(node));
    }
    pub(in crate::buffer::storage::provided) fn has_returned(&self) -> bool {
        let queue = self.returned.take();
        let ready = queue.is_some();
        self.returned.set(queue);
        ready
    }
    pub(super) fn finish(&self, node: arena::Ready<'d>) {
        // SAFETY: the node retains its stable slot for the complete scope.
        let tail = &unsafe { node.entry.as_ref() }.returned;
        let head = match self.returned.take() {
            Some((head, last)) => {
                last.set(Some(node));
                head
            }
            None => node,
        };
        self.returned.set(Some((head, tail)));
    }
}

const _: () = assert!(size_of::<State<'_>>() == 5 * size_of::<usize>());

type Queue<'d> = Option<(arena::Ready<'d>, &'d cell::Cell<Option<arena::Ready<'d>>>)>;

const _: () = assert!(size_of::<Queue<'_>>() == 2 * size_of::<usize>());

type SlotDomain<'d, T, L> = marker::PhantomData<fn(arena::Slot<'d, T, L>) -> arena::Slot<'d, T, L>>;
