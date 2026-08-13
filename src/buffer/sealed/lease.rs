use std::{alloc, marker, mem, ptr, slice};

use crate::buffer::{
    self, pool,
    pool::state,
    sealed::{self, core},
    write,
};

pub struct Lease<S: state::State = state::Uninitialized, C: pool::Capacity = pool::RuntimeCapacity>
{
    core: ptr::NonNull<core::Core>,
    index: u32,
    len: u32,
    marker: marker::PhantomData<(S, C, *mut ())>,
}

impl<S: state::State, C: pool::Capacity> Lease<S, C> {
    pub(super) fn new(core: ptr::NonNull<core::Core>, index: u32) -> Self {
        Self {
            core,
            index,
            len: 0,
            marker: marker::PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: this lease owns `index` and `0..len` is initialized.
        unsafe { slice::from_raw_parts(core::Core::data(self.core, self.index), self.len()) }
    }

    pub fn capacity(&self) -> usize {
        // SAFETY: the live lease retains its core.
        unsafe { self.core.as_ref() }.capacity as usize
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: `&mut self` uniquely borrows the lease slot and initialized prefix.
        unsafe { slice::from_raw_parts_mut(core::Core::data(self.core, self.index), self.len()) }
    }

    pub fn freeze(self) -> sealed::Frozen {
        let this = mem::ManuallyDrop::new(self);
        sealed::Frozen::new(this.core, this.index, this.len)
    }
}

impl<C: pool::Capacity> Lease<state::Uninitialized, C> {
    pub fn try_push(&mut self, byte: u8) -> Result<(), buffer::CapacityError> {
        let written = self.len();
        let capacity = self.capacity();
        if written == capacity {
            return Err(buffer::CapacityError::new(
                written.saturating_add(1),
                capacity,
            ));
        }
        // SAFETY: `written < capacity` selects the next byte in this live slot.
        unsafe {
            core::Core::data(self.core, self.index)
                .add(written)
                .cast::<mem::MaybeUninit<u8>>()
                .write(mem::MaybeUninit::new(byte));
        }
        self.len += 1;
        Ok(())
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        let start = self.len();
        let capacity = self.capacity();
        let end = start
            .checked_add(src.len())
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(buffer::CapacityError::new(end, capacity));
        }
        // SAFETY: the complete destination range was checked and the exclusive
        // lease prevents it from aliasing the borrowed source.
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                core::Core::data(self.core, self.index).add(start),
                src.len(),
            )
        };
        self.len = end as u32;
        Ok(())
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        let start = self.len();
        let end = buffer::checked_append_len(start, self.capacity(), &slices)?;
        let mut offset = start;
        for src in slices {
            // SAFETY: aggregate validation covers every disjoint destination.
            unsafe {
                ptr::copy_nonoverlapping(
                    src.as_ptr(),
                    core::Core::data(self.core, self.index).add(offset),
                    src.len(),
                )
            };
            offset += src.len();
        }
        self.len = end as u32;
        Ok(())
    }

    pub fn spare_writer(&mut self) -> write::SpareWriter<'_> {
        let capacity = unsafe { self.core.as_ref() }.capacity as usize;
        let written = self.len as usize;
        // SAFETY: this live lease uniquely owns the uninitialized suffix.
        let spare = unsafe {
            slice::from_raw_parts_mut(
                core::Core::data(self.core, self.index).add(written).cast(),
                capacity - written,
            )
        };
        write::SpareWriter::new(spare, &mut self.len)
    }
}

impl<C: pool::Capacity> Lease<state::Initialized, C> {
    /// Returns initialized capacity after the logical end.
    ///
    /// Reacquired slots retain values written by their previous lease.
    pub fn spare_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        let remaining = self.capacity() - len;
        // SAFETY: initialized-state slots retain initialized bytes through capacity.
        let bytes = unsafe {
            slice::from_raw_parts_mut(core::Core::data(self.core, self.index), self.capacity())
        };
        &mut bytes[len..len + remaining]
    }

    /// Extends the logical length into the initialized spare capacity.
    pub fn try_advance(&mut self, additional: usize) -> Result<(), buffer::CapacityError> {
        let len = self.len();
        let capacity = self.capacity();
        let attempted = len
            .checked_add(additional)
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(buffer::CapacityError::new(attempted, capacity));
        }
        self.len = attempted as u32;
        Ok(())
    }
}

impl<S: state::State, C: pool::Capacity> Drop for Lease<S, C> {
    fn drop(&mut self) {
        // SAFETY: this lease owns one live reference to its exact slot.
        let core = unsafe { self.core.as_ref() };
        let slot = unsafe { &*core::Core::slot(self.core, self.index) };
        if !slot.refs.release() {
            return;
        }
        slot.refs.deactivate();
        slot.next.set(core.free.get());
        core.free.set(self.index);
        core.free_len.set(core.free_len.get() + 1);
        if !core.refs.release() {
            return;
        }
        // SAFETY: the released slot also held the final core reference.
        let layout = unsafe {
            alloc::Layout::from_size_align_unchecked(core.allocation_size, align_of::<core::Core>())
        };
        unsafe { alloc::dealloc(self.core.as_ptr().cast(), layout) };
    }
}

impl<S: state::State, C: pool::Capacity> buffer::PrefixLength for Lease<S, C> {
    fn prefix_len(&self) -> usize {
        self.len as usize
    }
}
