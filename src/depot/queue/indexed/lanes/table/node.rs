use std::{cell::Cell, mem, ptr};

use crate::depot::{
    array::Index,
    queue::{indexed::lanes, links},
};

pub struct Node<V, const N: usize, S: lanes::raw::State<V> = ()> {
    pub(in crate::depot) lane: Cell<Option<Index<N>>>,
    pub(in crate::depot) hook: links::Hook,
    pub(in crate::depot) value: mem::ManuallyDrop<V>,
    pub(in crate::depot) state: S,
}

impl<V, const N: usize, S: lanes::raw::State<V>> Node<V, N, S> {
    pub(in crate::depot) fn new(index: links::Index, value: lanes::raw::Owned<V, S>) -> Self {
        Self::from_parts(None, links::Hook::new(index), value)
    }

    pub(in crate::depot) fn from_parts(
        lane: Option<Index<N>>,
        hook: links::Hook,
        value: lanes::raw::Owned<V, S>,
    ) -> Self {
        let (value, state) = value.into_parts();
        Self {
            lane: Cell::new(lane),
            hook,
            value: mem::ManuallyDrop::new(value),
            state,
        }
    }

    pub(in crate::depot) fn unpack(
        self,
    ) -> (Option<Index<N>>, links::Hook, lanes::raw::Owned<V, S>) {
        let mut node = mem::ManuallyDrop::new(self);
        // SAFETY: the consumed node owns its exact initialized payload, metadata,
        // and hook. Each non-Copy field moves once and its former Drop is suppressed.
        unsafe {
            let value = mem::ManuallyDrop::take(&mut node.value);
            let hook = ptr::read(&node.hook);
            let lane = node.lane.get();
            (lane, hook, lanes::raw::Owned::from_parts(value, node.state))
        }
    }
}

impl<V, const N: usize, S: lanes::raw::State<V>> Drop for Node<V, N, S> {
    fn drop(&mut self) {
        // SAFETY: each resident node owns its payload and matching destruction
        // state; retirement suppresses this Drop when transferring that ownership.
        unsafe { self.state.clear(&mut self.value) };
    }
}
