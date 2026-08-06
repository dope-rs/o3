use std::{alloc, num::NonZeroU32};

use crate::buffer::{
    PoolLayoutError,
    pool::core::{Core, Slot},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    allocation: alloc::Layout,
    slots: u32,
    capacity: NonZeroU32,
    data_offset: usize,
}

impl Layout {
    pub fn new(slots: usize, capacity: usize) -> Result<Self, PoolLayoutError> {
        let slots = u32::try_from(slots).map_err(|_| PoolLayoutError::SlotOverflow)?;
        let capacity = u32::try_from(capacity)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(if capacity == 0 {
                PoolLayoutError::ZeroCapacity
            } else {
                PoolLayoutError::CapacityOverflow
            })?;
        let slots_layout = alloc::Layout::array::<Slot>(slots as usize)
            .map_err(|_| PoolLayoutError::CapacityOverflow)?;
        let data_len = (slots as usize)
            .checked_mul(capacity.get() as usize)
            .ok_or(PoolLayoutError::CapacityOverflow)?;
        let data_layout =
            alloc::Layout::array::<u8>(data_len).map_err(|_| PoolLayoutError::CapacityOverflow)?;
        let (layout, _) = alloc::Layout::new::<Core>()
            .extend(slots_layout)
            .map_err(|_| PoolLayoutError::CapacityOverflow)?;
        let (layout, data_offset) = layout
            .extend(data_layout)
            .map_err(|_| PoolLayoutError::CapacityOverflow)?;
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
            let slot_bytes = SLOTS as u128 * size_of::<Slot>() as u128;
            let data_bytes = SLOTS as u128 * CAPACITY as u128;
            let padding = align_of::<Core>() as u128 + align_of::<Slot>() as u128;
            let total = size_of::<Core>() as u128 + slot_bytes + data_bytes + padding;
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
            let slot_bytes = SLOTS as u128 * size_of::<Slot>() as u128;
            let data_bytes = SLOTS as u128 * CAPACITY as u128;
            let padding = align_of::<Core>() as u128 + align_of::<Slot>() as u128;
            let total = size_of::<Core>() as u128 + slot_bytes + data_bytes + padding;
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

    pub(super) const fn capacity(self) -> NonZeroU32 {
        self.capacity
    }

    pub(super) const fn data_offset(self) -> usize {
        self.data_offset
    }

    pub(super) const fn slot_count(self) -> u32 {
        self.slots
    }
}
