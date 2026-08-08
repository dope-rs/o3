use std::{alloc, num};

use crate::buffer::pool::{self, core};

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
