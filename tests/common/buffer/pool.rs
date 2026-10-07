//! Initialized foreign-output fixtures.

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::{BorrowError, Cell, RefCell},
    num::NonZero,
};

use o3::{
    alloc, buffer,
    buffer::{pool, write::consume},
};

use crate::common::alloc::system::Tracker;

thread_local! {
    pub static REENTRANT: RefCell<Option<pool::Growing>> = const { RefCell::new(None) };
    static ARMED: Cell<(bool, Result<(), Error>)> = const { Cell::new((false, Ok(()))) };
}

#[derive(Debug)]
pub enum Error {
    Borrow(BorrowError),
    Allocation(alloc::Exhausted),
}

pub struct Reentrant;

impl Reentrant {
    pub fn on_alloc<T>(run: impl FnOnce() -> T) -> (T, Result<(), Error>) {
        let previous = ARMED.replace((true, Ok(())));
        let value = run();
        (value, ARMED.replace(previous).1)
    }

    fn before() {
        let (armed, mut observed) = ARMED.replace((false, Ok(())));
        if armed {
            observed = REENTRANT.with(|owner| {
                if let Some(owner) = owner.try_borrow().map_err(Error::Borrow)?.as_ref() {
                    drop(owner.try_borrow().map_err(Error::Allocation)?);
                }
                Ok(())
            });
        }
        ARMED.set((false, observed));
    }
}

// SAFETY: every request is forwarded unchanged to Tracker. The one-shot arm is
// cleared before accessing the retained pool, so nested allocation cannot reenter.
unsafe impl GlobalAlloc for Reentrant {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Self::before();
        unsafe { Tracker.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Self::before();
        unsafe { Tracker.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { Tracker.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        Self::before();
        unsafe { Tracker.realloc(pointer, layout, size) }
    }
}

/// Fills and completes the admitted nonzero prefix of the same initialized output.
pub trait Fill: Sized {
    type Complete;

    fn try_fill<const N: usize>(
        self,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<Self::Complete, consume::Error>;
}

impl<'a, L> Fill for buffer::raw::Output<'a, L> {
    type Complete = pool::Complete<'a, L>;

    fn try_fill<const N: usize>(
        mut self,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<Self::Complete, consume::Error> {
        const { assert!(N > 0) };
        consume::Error::check(N, self.len())?;
        fill(&mut self.as_mut_slice()[..N]);
        // SAFETY: N is nonzero and the same exclusive initialized output retains
        // the checked N-byte prefix throughout the fixture's write.
        Ok(unsafe { self.complete(NonZero::new_unchecked(N)) })
    }
}
