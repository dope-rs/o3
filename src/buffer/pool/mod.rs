use std::{error::Error, fmt, marker::PhantomData, ptr::NonNull};

use crate::buffer;

mod core;
mod cursor;
mod frozen;
mod layout;
mod lease;
mod plan;
pub mod state;

pub use cursor::Cursor;
pub use frozen::Frozen;
pub use layout::Layout;
pub use lease::Lease;
pub use plan::Plan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    ZeroCapacity,
    SlotOverflow,
    CapacityOverflow,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => f.write_str("buffer pool capacity must be positive"),
            Self::SlotOverflow => f.write_str("buffer pool slot count overflow"),
            Self::CapacityOverflow => f.write_str("buffer pool allocation size overflow"),
        }
    }
}

impl Error for LayoutError {}

/// Selects whether a pool capacity is fixed in its type or stored in its layout.
///
/// This bound is sealed because the allocator's layout proof depends on it.
#[doc(hidden)]
pub trait Capacity: buffer::Seal {}

#[repr(transparent)]
pub struct Pool<S: state::State = state::Uninitialized, C: Capacity = RuntimeCapacity> {
    core: NonNull<core::Core>,
    marker: PhantomData<(S, C, *mut ())>,
}

impl<S: state::State> Pool<S, RuntimeCapacity> {
    pub fn from_layout(layout: Layout) -> Self {
        Self {
            core: core::Core::allocate::<S>(layout),
            marker: PhantomData,
        }
    }

    pub fn try_new(slots: usize, capacity: usize) -> Result<Self, LayoutError> {
        Ok(Self::from_layout(Layout::new(slots, capacity)?))
    }
}

impl<S: state::State, C: Capacity> Pool<S, C> {
    pub fn try_acquire(&self) -> Option<Lease<S, C>> {
        let index = core::Core::acquire(self.core)?;
        Some(Lease {
            core: self.core,
            index,
            len: 0,
            marker: PhantomData,
        })
    }

    pub fn capacity(&self) -> usize {
        core::Core::capacity(self.core)
    }

    pub fn available(&self) -> usize {
        core::Core::available(self.core)
    }
}

impl<S: state::State, const CAP: u32> Pool<S, FixedCapacity<CAP>> {
    pub fn try_with_slots(slots: usize) -> Result<Self, LayoutError> {
        let layout = Layout::new(slots, CAP as usize)?;
        Ok(Self {
            core: core::Core::allocate::<S>(layout),
            marker: PhantomData,
        })
    }

    #[must_use]
    pub fn fixed<const SLOTS: usize>() -> Self {
        let layout = Layout::fixed_capacity::<SLOTS, CAP>();
        Self {
            core: core::Core::allocate::<S>(layout),
            marker: PhantomData,
        }
    }
}

impl<C: Capacity> Pool<state::Uninitialized, C> {
    #[must_use]
    pub fn try_acquire_buffer(&self) -> Option<Cursor<C>> {
        self.try_acquire().map(Cursor::new)
    }
}

impl<S: state::State, C: Capacity> Clone for Pool<S, C> {
    fn clone(&self) -> Self {
        core::Core::retain(self.core);
        Self {
            core: self.core,
            marker: PhantomData,
        }
    }
}

impl<S: state::State, C: Capacity> Drop for Pool<S, C> {
    fn drop(&mut self) {
        core::Core::release(self.core);
    }
}

/// Selects a capacity supplied by [`Layout`] at construction time.
#[derive(Clone, Copy)]
pub struct RuntimeCapacity;

impl buffer::Seal for RuntimeCapacity {}

impl Capacity for RuntimeCapacity {}

/// Selects a capacity fixed in the pool type.
#[derive(Clone, Copy)]
pub struct FixedCapacity<const CAP: u32>;

impl<const CAP: u32> buffer::Seal for FixedCapacity<CAP> {}

impl<const CAP: u32> Capacity for FixedCapacity<CAP> {}
