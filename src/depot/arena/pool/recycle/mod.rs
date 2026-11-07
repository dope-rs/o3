use std::{cell, mem, pin, ptr};

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
pub struct Header<T, C = ()> {
    pub(in crate::depot::arena::pool) free: cell::Cell<*mut pool::Entry<T, C>>,
    pub(in crate::depot::arena::pool) context: mem::ManuallyDrop<C>,
}

/// Fixed initialized storage; leases return slots without changing their contents.
#[repr(transparent)]
pub struct Store<T> {
    block: tail::Block<Header<T>, pool::Entry<T>, capacity::Count>,
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
        init: impl FnMut(u32) -> T,
    ) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            block: pool::Entry::init(
                tail::Plan::try_new(capacity)?.try_allocate()?,
                Header {
                    free: cell::Cell::new(ptr::null_mut()),
                    context: mem::ManuallyDrop::new(()),
                },
                init,
            ),
        })
    }

    pub fn vacancies(&self) -> &Header<T> {
        tail::View::new(&self.block).header()
    }

    pub fn take(&self) -> Option<pool::Unique<'_, T>> {
        self.vacancies().take()
    }
}

impl<T> Header<T> {
    pub fn take(&self) -> Option<pool::Unique<'_, T>> {
        Some(pool::Vacant::new(self)?.reserve())
    }
}
