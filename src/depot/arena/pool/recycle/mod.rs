use std::{cell, pin, ptr};

use crate::{
    alloc::tail,
    depot::{arena::pool, capacity},
};

pub mod release;

/// Resets a pinned value in place before its slot is recycled.
pub trait Recycle {
    type Seed: Copy;
    type Release;

    fn from_seed(seed: Self::Seed) -> Self;
    fn recycle(self: pin::Pin<&mut Self>) -> Self::Release;
}

#[derive(Debug)]
pub struct Header<T> {
    pub(in crate::depot::arena::pool) free: cell::Cell<*mut pool::Entry<T, Header<T>>>,
}

/// Fixed initialized storage; leases return slots without changing their contents.
#[repr(transparent)]
pub struct Store<T> {
    block: tail::Block<Header<T>, pool::Entry<T, Header<T>>, capacity::Count>,
}

impl<T> Store<T> {
    pub fn try_new(capacity: capacity::Count) -> Result<Self, crate::alloc::Error>
    where
        T: Default,
    {
        Self::try_from_fn(capacity, |_| T::default())
    }

    /// Visits every index in `0..capacity` exactly once in order before publishing slots.
    /// The retained extent also admits zero residents without invoking `init`.
    pub fn try_from_fn(
        capacity: capacity::Count,
        mut init: impl FnMut(u32) -> T,
    ) -> Result<Self, crate::alloc::Error> {
        let plan = tail::Plan::try_new(capacity)?;
        let memory = plan.try_allocate()?;
        let mut block = memory.fill(
            Header {
                free: cell::Cell::new(ptr::null_mut()),
            },
            |_, index| pool::Entry::new(init(index)),
        );
        let (header, entries) = block.parts_mut();
        // SAFETY: this constructor exclusively owns the freshly allocated block,
        // so every entry is linked into the empty free list exactly once.
        unsafe { pool::Entry::chain(entries, &header.free) };
        Ok(Self { block })
    }

    pub fn vacancies(&self) -> &Header<T> {
        tail::View::new(&self.block).header()
    }

    pub fn take(&self) -> Option<pool::Unique<'_, T, Header<T>>> {
        self.vacancies().take()
    }
}

impl<T> Header<T> {
    pub fn take(&self) -> Option<pool::Unique<'_, T, Self>> {
        Some(pool::Vacant::new(self)?.reserve())
    }

    pub(in crate::depot) fn free_list(&self) -> &cell::Cell<*mut pool::Entry<T, Self>> {
        &self.free
    }
}
