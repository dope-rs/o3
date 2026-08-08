use std::{marker, ptr};

use crate::buffer::{
    self,
    pool::{self, core, state},
    write,
};

pub struct Lease<S: state::State = state::Uninitialized, C: pool::Capacity = pool::RuntimeCapacity>
{
    pub(super) core: ptr::NonNull<core::Core>,
    pub(super) index: u32,
    pub(super) len: u32,
    pub(super) marker: marker::PhantomData<(S, C, *mut ())>,
}

impl<S: state::State, C: pool::Capacity> Lease<S, C> {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        core::Core::slice(self.core, self.index, self.len())
    }

    pub fn capacity(&self) -> usize {
        core::Core::capacity(self.core)
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        core::Core::slice_mut(self.core, self.index, self.len())
    }

    pub fn freeze(self) -> pool::Frozen {
        use std::mem::ManuallyDrop;

        let this = ManuallyDrop::new(self);
        pool::Frozen {
            core: this.core,
            index: this.index,
            len: this.len,
            marker: marker::PhantomData,
        }
    }
}

impl<C: pool::Capacity> Lease<state::Uninitialized, C> {
    pub fn try_push(&mut self, byte: u8) -> Result<(), buffer::CapacityError> {
        core::Core::push(self.core, self.index, &mut self.len, byte)
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        core::Core::extend(self.core, self.index, &mut self.len, src)
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        core::Core::extend_from_slices(self.core, self.index, &mut self.len, slices)
    }

    pub fn spare_writer(&mut self) -> write::SpareWriter<'_> {
        core::Core::spare_writer(self.core, self.index, &mut self.len)
    }
}

impl<C: pool::Capacity> Lease<state::Initialized, C> {
    /// Returns initialized capacity after the logical end.
    ///
    /// Reacquired slots retain values written by their previous lease.
    pub fn spare_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        let remaining = self.capacity() - len;
        let bytes = core::Core::slice_mut(self.core, self.index, self.capacity());
        &mut bytes[len..len + remaining]
    }

    /// Extends the logical length into the initialized spare capacity.
    pub fn try_advance(&mut self, additional: usize) -> Result<(), buffer::CapacityError> {
        use crate::buffer::CapacityError;
        let len = self.len();
        let capacity = self.capacity();
        let attempted = len
            .checked_add(additional)
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(CapacityError::new(attempted, capacity));
        }
        self.len = attempted as u32;
        Ok(())
    }
}

impl<S: state::State, C: pool::Capacity> Drop for Lease<S, C> {
    fn drop(&mut self) {
        core::Core::release_slot(self.core, self.index);
    }
}

impl<S: state::State, C: pool::Capacity> buffer::PrefixLength for Lease<S, C> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}
