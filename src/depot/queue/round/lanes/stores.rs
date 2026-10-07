use std::{cell, marker, mem, num};

use crate::{
    alloc,
    depot::{capacity, queue::links},
};

#[derive(Debug)]
#[repr(C)]
pub(in crate::depot::queue::round) struct Node<T> {
    pub(in crate::depot::queue::round) value: T,
    pub(in crate::depot::queue::round) hook: links::Hook,
}

#[derive(Debug)]
#[repr(C)]
pub(in crate::depot::queue::round) struct Track {
    pub(in crate::depot::queue::round) entries: links::Chain,
    pub(in crate::depot::queue::round) active: links::Hook,
}

impl Track {
    pub(in crate::depot::queue::round) fn new(index: links::Index) -> Self {
        Self {
            entries: links::Chain::new(),
            active: links::Hook::new(index),
        }
    }
}

#[derive(Debug)]
#[repr(C)]
pub(in crate::depot::queue::round) struct Capacity<T> {
    count: capacity::Count,
    lanes: num::NonZero<u32>,
    ownership: marker::PhantomData<(Node<T>, Track, cell::Cell<links::Index>)>,
}

impl<T> Capacity<T> {
    pub(in crate::depot::queue::round) fn new(
        capacity: usize,
        lanes: num::NonZero<usize>,
    ) -> Result<Self, alloc::Overflow> {
        const {
            assert!(mem::align_of::<Node<T>>() >= mem::align_of::<Track>());
            assert!(mem::align_of::<Track>() >= mem::align_of::<cell::Cell<links::Index>>());
        }
        let count = capacity::Count::try_count(capacity, alloc::Overflow)?;
        let lanes = capacity::Count::try_narrow(lanes, alloc::Overflow)?;
        alloc::raw::Bundle::try_layout::<Self>(count.get(), lanes.get() as usize)?;
        Ok(Self {
            count,
            lanes,
            ownership: marker::PhantomData,
        })
    }
}

unsafe impl<T> alloc::raw::Shape for Capacity<T> {
    type Root = Node<T>;
    type Tail = Track;
    type Extra = cell::Cell<links::Index>;
    type Last = ();

    fn len(&self) -> usize {
        self.count.get()
    }

    fn tail_len(&self) -> usize {
        self.lanes.get() as usize
    }
}

unsafe impl<T> crate::depot::raw::Hooks for [Node<T>] {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        unsafe { &self.get_unchecked(index.get()).hook }
    }
}

unsafe impl crate::depot::raw::Hooks for [Track] {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        unsafe { &self.get_unchecked(index.get()).active }
    }
}

const _: () = {
    assert!(mem::size_of::<Track>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<cell::Cell<links::Index>>() == mem::size_of::<u32>());
};
