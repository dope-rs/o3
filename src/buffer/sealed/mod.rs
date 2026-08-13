use std::{alloc, marker, ptr};

use crate::buffer::{pool, pool::state};

mod core;
mod frozen;
mod layout;
mod lease;

pub use frozen::Frozen;
pub use layout::{Layout, Plan};
pub use lease::Lease;

pub trait Seal {}

#[repr(transparent)]
pub struct Pool<S: state::State = state::Uninitialized, C: pool::Capacity = pool::RuntimeCapacity> {
    core: ptr::NonNull<core::Core>,
    marker: marker::PhantomData<(S, C, *mut ())>,
}

impl<S: state::State> Pool<S, pool::RuntimeCapacity> {
    pub fn from_layout(layout: Layout) -> Self {
        Self {
            core: core::Core::allocate::<S>(layout),
            marker: marker::PhantomData,
        }
    }

    pub fn try_new(slots: usize, capacity: usize) -> Result<Self, pool::LayoutError> {
        Ok(Self::from_layout(Layout::new(slots, capacity)?))
    }

    pub fn try_from_layout(layout: Layout) -> Result<Self, pool::AllocationError> {
        Ok(Self {
            core: core::Core::try_allocate::<S>(layout)?,
            marker: marker::PhantomData,
        })
    }
}

impl<S: state::State, C: pool::Capacity> Pool<S, C> {
    pub fn try_acquire(&self) -> Option<Lease<S, C>> {
        let index = core::Core::acquire_owned(self.core)?;
        Some(Lease::new(self.core, index))
    }

    /// Acquires a slot whose allocation lifetime is proven by this pool borrow.
    /// Unlike [`try_acquire`](Self::try_acquire), this does not retain the pool
    /// allocation.
    pub fn try_acquire_borrowed(&self) -> Option<Lease<S, C, pool::Borrowed<'_>>> {
        let index = core::Core::acquire_borrowed(self.core)?;
        Some(Lease::new(self.core, index))
    }

    pub fn capacity(&self) -> usize {
        // SAFETY: `Pool` owns one live reference to this core.
        unsafe { self.core.as_ref() }.capacity as usize
    }

    pub fn slots(&self) -> usize {
        // SAFETY: `Pool` owns one live reference to this core.
        unsafe { self.core.as_ref() }.slots as usize
    }

    pub fn available(&self) -> usize {
        // SAFETY: `Pool` owns one live reference to this core.
        unsafe { self.core.as_ref() }.free_len.get() as usize
    }
}

impl<S: state::State, const CAP: u32> Pool<S, pool::FixedCapacity<CAP>> {
    pub fn try_with_slots(slots: usize) -> Result<Self, pool::LayoutError> {
        let layout = Layout::new(slots, CAP as usize)?;
        Ok(Self {
            core: core::Core::allocate::<S>(layout),
            marker: marker::PhantomData,
        })
    }

    pub fn try_allocate_slots(slots: usize) -> Result<Self, pool::CreateError> {
        let layout = Layout::new(slots, CAP as usize)?;
        Ok(Self {
            core: core::Core::try_allocate::<S>(layout)?,
            marker: marker::PhantomData,
        })
    }

    #[must_use]
    pub fn fixed<const SLOTS: usize>() -> Self {
        let layout = Layout::fixed_capacity::<SLOTS, CAP>();
        Self {
            core: core::Core::allocate::<S>(layout),
            marker: marker::PhantomData,
        }
    }
}

impl<C: pool::Capacity> Pool<state::Uninitialized, C> {
    #[must_use]
    pub fn try_acquire_buffer(&self) -> Option<pool::Cursor<C>> {
        self.try_acquire().map(pool::Cursor::new)
    }

    #[must_use]
    pub fn try_acquire_borrowed_buffer(&self) -> Option<pool::BorrowedCursor<'_, C>> {
        self.try_acquire_borrowed().map(pool::BorrowedCursor::new)
    }
}

impl<S: state::State, C: pool::Capacity> Clone for Pool<S, C> {
    fn clone(&self) -> Self {
        // SAFETY: the source pool keeps the core allocation live.
        unsafe { self.core.as_ref() }.refs.retain();
        Self {
            core: self.core,
            marker: marker::PhantomData,
        }
    }
}

impl<S: state::State, C: pool::Capacity> Drop for Pool<S, C> {
    fn drop(&mut self) {
        // SAFETY: this pool owns one live core reference.
        let core = unsafe { self.core.as_ref() };
        if !core.refs.release() {
            return;
        }
        // SAFETY: allocation size was produced by `Layout` with Core alignment.
        let layout = unsafe {
            alloc::Layout::from_size_align_unchecked(core.allocation_size, align_of::<core::Core>())
        };
        // SAFETY: the final reference owns the allocation and exact layout.
        unsafe { alloc::dealloc(self.core.as_ptr().cast(), layout) };
    }
}
