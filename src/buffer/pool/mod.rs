use std::{marker::PhantomData, ptr::NonNull};

use crate::buffer::PoolLayoutError;

mod core;
mod cursor;
mod frozen;
mod layout;
mod lease;
mod plan;
mod state;

use core::Core;

pub use cursor::Cursor;
pub use frozen::Frozen;
pub use layout::Layout;
pub use lease::Lease;
pub use plan::Plan;
#[doc(hidden)]
pub use state::{Initialized, State, Uninitialized};

pub trait PoolCapacitySealed {}

/// Selects whether a pool capacity is fixed in its type or stored in its layout.
///
/// This bound is sealed because the allocator's layout proof depends on it.
#[doc(hidden)]
pub trait PoolCapacity: PoolCapacitySealed {}

#[repr(transparent)]
pub struct Pool<S: State = Uninitialized, C: PoolCapacity = RuntimePoolCapacity> {
    core: NonNull<Core>,
    marker: PhantomData<(S, C, *mut ())>,
}

impl<S: State> Pool<S, RuntimePoolCapacity> {
    pub fn from_layout(layout: Layout) -> Self {
        Self {
            core: Core::allocate::<S>(layout),
            marker: PhantomData,
        }
    }

    pub fn try_new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        Ok(Self::from_layout(Layout::new(slots, capacity)?))
    }
}

impl<S: State, C: PoolCapacity> Pool<S, C> {
    pub fn try_acquire(&self) -> Option<Lease<S, C>> {
        let index = Core::acquire(self.core)?;
        Some(Lease {
            core: self.core,
            index,
            len: 0,
            marker: PhantomData,
        })
    }

    pub fn capacity(&self) -> usize {
        Core::capacity(self.core)
    }

    pub fn available(&self) -> usize {
        Core::available(self.core)
    }
}

impl<S: State, const CAP: u32> Pool<S, FixedPoolCapacity<CAP>> {
    pub fn try_with_slots(slots: usize) -> Result<Self, PoolLayoutError> {
        let layout = Layout::new(slots, CAP as usize)?;
        Ok(Self {
            core: Core::allocate::<S>(layout),
            marker: PhantomData,
        })
    }

    #[must_use]
    pub fn fixed<const SLOTS: usize>() -> Self {
        let layout = Layout::fixed_capacity::<SLOTS, CAP>();
        Self {
            core: Core::allocate::<S>(layout),
            marker: PhantomData,
        }
    }
}

impl<C: PoolCapacity> Pool<Uninitialized, C> {
    #[must_use]
    pub fn try_acquire_buffer(&self) -> Option<Cursor<C>> {
        self.try_acquire().map(Cursor::new)
    }
}

impl<S: State, C: PoolCapacity> Clone for Pool<S, C> {
    fn clone(&self) -> Self {
        Core::retain(self.core);
        Self {
            core: self.core,
            marker: PhantomData,
        }
    }
}

impl<S: State, C: PoolCapacity> Drop for Pool<S, C> {
    fn drop(&mut self) {
        Core::release(self.core);
    }
}

/// Selects a capacity supplied by [`Layout`] at construction time.
#[derive(Clone, Copy)]
pub struct RuntimePoolCapacity;

impl PoolCapacitySealed for RuntimePoolCapacity {}

impl PoolCapacity for RuntimePoolCapacity {}

/// Selects a capacity fixed in the pool type.
#[derive(Clone, Copy)]
pub struct FixedPoolCapacity<const CAP: u32>;

impl<const CAP: u32> PoolCapacitySealed for FixedPoolCapacity<CAP> {}

impl<const CAP: u32> PoolCapacity for FixedPoolCapacity<CAP> {}
