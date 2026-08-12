use std::{alloc, marker, mem, num, ptr, slice};

use crate::buffer::{self, pool, pool::state, write};

mod core;

pub trait Seal {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    allocation: alloc::Layout,
    slots: u32,
    capacity: num::NonZeroU32,
    data_offset: usize,
}

impl Layout {
    pub fn new(slots: usize, capacity: usize) -> Result<Self, pool::LayoutError> {
        use crate::buffer::pool::LayoutError;
        let slots = u32::try_from(slots).map_err(|_| LayoutError::SlotOverflow)?;
        let capacity = u32::try_from(capacity)
            .ok()
            .and_then(num::NonZeroU32::new)
            .ok_or(if capacity == 0 {
                LayoutError::ZeroCapacity
            } else {
                LayoutError::CapacityOverflow
            })?;
        let slots_layout = alloc::Layout::array::<core::Slot>(slots as usize)
            .map_err(|_| LayoutError::CapacityOverflow)?;
        let data_len = (slots as usize)
            .checked_mul(capacity.get() as usize)
            .ok_or(LayoutError::CapacityOverflow)?;
        let data_layout =
            alloc::Layout::array::<u8>(data_len).map_err(|_| LayoutError::CapacityOverflow)?;
        let (layout, _) = alloc::Layout::new::<core::Core>()
            .extend(slots_layout)
            .map_err(|_| LayoutError::CapacityOverflow)?;
        let (layout, data_offset) = layout
            .extend(data_layout)
            .map_err(|_| LayoutError::CapacityOverflow)?;
        Ok(Self {
            allocation: layout.pad_to_align(),
            slots,
            capacity,
            data_offset,
        })
    }

    #[must_use]
    pub fn fixed<const SLOTS: usize, const CAPACITY: usize>() -> Self {
        const {
            assert!(SLOTS <= u32::MAX as usize);
            assert!(CAPACITY != 0);
            assert!(CAPACITY <= u32::MAX as usize);
            let slot_bytes = SLOTS as u128 * size_of::<core::Slot>() as u128;
            let data_bytes = SLOTS as u128 * CAPACITY as u128;
            let padding = align_of::<core::Core>() as u128 + align_of::<core::Slot>() as u128;
            let total = size_of::<core::Core>() as u128 + slot_bytes + data_bytes + padding;
            assert!(total <= isize::MAX as u128);
        }
        // SAFETY: the const proof covers every conversion and Layout size bound.
        unsafe { Self::new(SLOTS, CAPACITY).unwrap_unchecked() }
    }

    #[must_use]
    pub fn fixed_capacity<const SLOTS: usize, const CAPACITY: u32>() -> Self {
        const {
            assert!(SLOTS <= u32::MAX as usize);
            assert!(CAPACITY != 0);
            let slot_bytes = SLOTS as u128 * size_of::<core::Slot>() as u128;
            let data_bytes = SLOTS as u128 * CAPACITY as u128;
            let padding = align_of::<core::Core>() as u128 + align_of::<core::Slot>() as u128;
            let total = size_of::<core::Core>() as u128 + slot_bytes + data_bytes + padding;
            assert!(total <= isize::MAX as u128);
        }
        // SAFETY: the const proof covers every conversion and Layout size bound.
        unsafe { Self::new(SLOTS, CAPACITY as usize).unwrap_unchecked() }
    }

    pub const fn slots(self) -> usize {
        self.slots as usize
    }

    pub(super) const fn allocation(self) -> alloc::Layout {
        self.allocation
    }

    pub(super) const fn capacity(self) -> num::NonZeroU32 {
        self.capacity
    }

    pub(super) const fn data_offset(self) -> usize {
        self.data_offset
    }

    pub(super) const fn slot_count(self) -> u32 {
        self.slots
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Plan {
    max_slots: usize,
    capacity: usize,
}

impl Plan {
    pub fn new(max_slots: usize, capacity: usize) -> Result<Self, pool::LayoutError> {
        Layout::new(max_slots, capacity)?;
        Ok(Self {
            max_slots,
            capacity,
        })
    }

    #[must_use]
    pub fn fixed<const MAX_SLOTS: usize, const CAPACITY: usize>() -> Self {
        let _ = Layout::fixed::<MAX_SLOTS, CAPACITY>();
        Self {
            max_slots: MAX_SLOTS,
            capacity: CAPACITY,
        }
    }

    pub fn layout_up_to(self, requested: usize) -> Layout {
        let slots = requested.min(self.max_slots);
        // SAFETY: the maximum layout was validated and reducing slots cannot overflow it.
        unsafe { Layout::new(slots, self.capacity).unwrap_unchecked() }
    }

    pub const fn max_slots(self) -> usize {
        self.max_slots
    }
}

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
        let index = core::Core::acquire(self.core)?;
        Some(Lease {
            core: self.core,
            index,
            len: 0,
            marker: marker::PhantomData,
        })
    }

    pub fn capacity(&self) -> usize {
        // SAFETY: `Pool` owns one live reference to this core.
        unsafe { self.core.as_ref() }.capacity as usize
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

pub struct Lease<S: state::State = state::Uninitialized, C: pool::Capacity = pool::RuntimeCapacity>
{
    core: ptr::NonNull<core::Core>,
    index: u32,
    len: u32,
    marker: marker::PhantomData<(S, C, *mut ())>,
}

impl<S: state::State, C: pool::Capacity> Lease<S, C> {
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

    pub fn freeze(self) -> Frozen {
        use std::mem::ManuallyDrop;

        let this = ManuallyDrop::new(self);
        Frozen {
            core: this.core,
            index: this.index,
            len: this.len,
            marker: marker::PhantomData,
        }
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

pub struct Frozen {
    core: ptr::NonNull<core::Core>,
    index: u32,
    len: u32,
    marker: marker::PhantomData<*mut ()>,
}

impl Frozen {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn capacity(&self) -> usize {
        // SAFETY: the frozen handle retains its slot and core.
        unsafe { self.core.as_ref() }.capacity as usize
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the frozen slot remains live and `0..len` is initialized.
        unsafe { slice::from_raw_parts(core::Core::data(self.core, self.index), self.len()) }
    }
}

impl Clone for Frozen {
    fn clone(&self) -> Self {
        // SAFETY: the source keeps the exact slot live.
        let slot = unsafe { &*core::Core::slot(self.core, self.index) };
        slot.refs.retain();
        Self {
            core: self.core,
            index: self.index,
            len: self.len,
            marker: marker::PhantomData,
        }
    }
}

impl AsRef<[u8]> for Frozen {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        // SAFETY: this handle owns one reference to its exact slot.
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
