use std::{cell, mem};

use crate::depot::queue::ring::inline::index;

pub(in crate::depot::queue::ring::inline) struct Slot<T, const N: usize, P> {
    pub(in crate::depot::queue::ring::inline) value: cell::UnsafeCell<T>,
    pub(in crate::depot::queue::ring::inline) payload: cell::UnsafeCell<mem::MaybeUninit<P>>,
    pub(in crate::depot::queue::ring::inline) next: cell::Cell<Option<index::Index<N>>>,
}
