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
    pub(super) fn new(capacity: num::NonZero<u32>, limit: u32, setting: L::Setting) -> Self
    where
        L: arena::Layout,
    {
        Self {
            state: State::new(capacity, limit.min(capacity.get()), L::mode(setting)),
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
        let (queue, node) = self.state.returned.replace(Queue::Empty).pop();
        self.state.returned.set(queue);
        node.map(|node| arena::Returned {
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
    mode: provided::Mode,
    pub(super) limit: u32,
    brand: marker::PhantomData<fn(&'d ()) -> &'d ()>,
}

impl<'d> State<'d> {
    const fn new(capacity: num::NonZero<u32>, limit: u32, mode: provided::Mode) -> Self {
        Self {
            free: cell::Cell::new(None),
            returned: cell::Cell::new(Queue::Empty),
            accounted: Counter::new(),
            capacity,
            mode,
            limit,
            brand: marker::PhantomData,
        }
    }
    pub(in crate::buffer::storage::provided) const fn capacity(&self) -> num::NonZero<u32> {
        self.capacity
    }
    pub(super) const fn mode(&self) -> provided::Mode {
        self.mode
    }
    pub(super) fn push_free(&self, node: arena::Empty<'d>) {
        let next = self.free.take();
        // SAFETY: this affine vacancy retains its exact detached slot.
        unsafe { node.entry.as_ref() }.free.set(next);
        self.free.set(Some(node));
    }
    pub(in crate::buffer::storage::provided) fn has_returned(&self) -> bool {
        let queue = self.returned.replace(Queue::Empty);
        let ready = !matches!(queue, Queue::Empty);
        self.returned.set(queue);
        ready
    }
    pub(super) fn finish(&self, node: arena::Ready<'d>) {
        let queue = self.returned.replace(Queue::Empty).push(node);
        self.returned.set(queue);
    }
}

const _: () = assert!(size_of::<State<'_>>() == 5 * size_of::<usize>());

enum Queue<'d> {
    Empty,
    Live {
        head: arena::Ready<'d>,
        tail: &'d cell::Cell<Option<arena::Ready<'d>>>,
    },
}

impl<'d> Queue<'d> {
    fn pop(self) -> (Self, Option<arena::Ready<'d>>) {
        let Self::Live { mut head, tail } = self else {
            return (Self::Empty, None);
        };
        let queue = match head.take_next() {
            Some(next) => Self::Live { head: next, tail },
            None => Self::Empty,
        };
        (queue, Some(head))
    }

    fn push(self, node: arena::Ready<'d>) -> Self {
        // SAFETY: the node retains its stable slot for the complete scope.
        let tail = &unsafe { node.entry.as_ref() }.returned;
        match self {
            Self::Empty => Self::Live { head: node, tail },
            Self::Live { head, tail: last } => {
                last.set(Some(node));
                Self::Live { head, tail }
            }
        }
    }
}

const _: () = assert!(size_of::<Queue<'_>>() == 2 * size_of::<usize>());

type SlotDomain<'d, T, L> = marker::PhantomData<fn(arena::Slot<'d, T, L>) -> arena::Slot<'d, T, L>>;
