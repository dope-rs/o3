use std::{mem, num};

use crate::{
    alloc,
    buffer::{pool, pool::owners, resident::staging},
};

pub(super) mod fixed;
pub(super) mod width;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Layout {
    slots: u32,
    capacity: num::NonZero<u32>,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(in crate::buffer) struct Tally {
    layout: Layout,
    count: u32,
}

struct Bounds {
    bytes: u128,
    offset: u128,
}

impl Bounds {
    const fn new(slots: u32, capacity: num::NonZero<u32>) -> Self {
        let count = slots as u128;
        let cells = count * mem::size_of::<owners::Slot>() as u128;
        Self {
            bytes: count * capacity.get() as u128,
            offset: mem::size_of::<owners::Header>() as u128 + cells,
        }
    }

    const fn fits_bytes(&self) -> bool {
        self.bytes <= isize::MAX as u128
    }
    const fn fits_data(&self) -> bool {
        self.offset + self.bytes <= alloc::raw::Layout::<owners::Header>::MAX
    }
}

impl Layout {
    const fn fits_count(count: usize) -> bool {
        count <= u32::MAX as usize
    }

    pub const fn new(slots: usize, capacity: usize) -> Result<Self, pool::LayoutError> {
        if !Self::fits_count(slots) {
            return Err(pool::LayoutError::SlotsTooLarge);
        }
        if capacity == 0 {
            return Err(pool::LayoutError::ZeroCapacity);
        }
        if !Self::fits_count(capacity) {
            return Err(pool::LayoutError::CapacityTooLarge);
        }
        let slots = slots as u32;
        let capacity = num::NonZero::<u32>::MIN.saturating_add(capacity as u32 - 1);
        Self::try_capacity(slots, capacity)
    }

    pub(in crate::buffer) const fn try_capacity(
        slots: u32,
        capacity: num::NonZero<u32>,
    ) -> Result<Self, pool::LayoutError> {
        let bounds = Bounds::new(slots, capacity);
        if !bounds.fits_bytes() {
            return Err(pool::LayoutError::PlainDataArray {
                slots,
                capacity: capacity.get(),
            });
        }
        if !bounds.fits_data() {
            return Err(pool::LayoutError::PlainData {
                slots,
                capacity: capacity.get(),
            });
        }
        Ok(Self { slots, capacity })
    }

    /// Builds a dynamic layout from compile-time dimensions.
    #[must_use]
    pub const fn constant<const SLOTS: usize, const CAPACITY: usize>() -> Self {
        const {
            match Self::new(SLOTS, CAPACITY) {
                Ok(layout) => layout,
                Err(_) => panic!("layout"),
            }
        }
    }

    const fn assert_valid<const SLOTS: usize, const CAPACITY: usize>() {
        assert!(Self::new(SLOTS, CAPACITY).is_ok(), "layout");
    }

    pub const fn slots(self) -> u32 {
        self.slots
    }

    /// Returns total slot payload bytes, excluding pool metadata and padding.
    pub const fn bytes(self) -> usize {
        (self.slots as u64 * self.capacity.get() as u64) as usize
    }

    pub(super) const fn capacity(self) -> num::NonZero<u32> {
        self.capacity
    }
}

impl Tally {
    pub(in crate::buffer) const fn into_parts(self) -> (Layout, u32) {
        (self.layout, self.count)
    }
}

impl<const SLOTS: usize, const CAPACITY: usize> From<fixed::Fixed<SLOTS, CAPACITY>> for Layout {
    fn from(fixed::Fixed(()): fixed::Fixed<SLOTS, CAPACITY>) -> Self {
        Self::constant::<SLOTS, CAPACITY>()
    }
}

impl<const CAPACITY: usize> From<width::Width<CAPACITY>> for Layout {
    fn from(width::Width(slots): width::Width<CAPACITY>) -> Self {
        Self {
            slots,
            capacity: num::NonZero::<u32>::MIN.saturating_add(CAPACITY as u32 - 1),
        }
    }
}

impl From<NonEmpty> for Tally {
    fn from(layout: NonEmpty) -> Self {
        Self {
            layout: layout.into_layout(),
            count: 1,
        }
    }
}

impl From<staging::Layout> for Tally {
    fn from(layout: staging::Layout) -> Self {
        Self {
            layout: Layout {
                slots: layout.warm_slots().get(),
                capacity: layout.plan().warm(),
            },
            count: layout.accounts(),
        }
    }
}

const _: () = {
    let count = u32::MAX as u128;
    let cells = count * size_of::<owners::Slot>() as u128;
    assert!(count * count <= usize::MAX as u128);
    assert!(cells <= alloc::raw::Layout::<owners::Slot>::MAX);
    assert!(
        size_of::<owners::Header>() as u128 + cells <= alloc::raw::Layout::<owners::Header>::MAX
    );
    assert!(size_of::<Layout>() == size_of::<u64>());
    assert!(size_of::<Tally>() == 3 * size_of::<u32>());
    assert!(size_of::<Result<Layout, pool::LayoutError>>() == 16);
};

#[derive(Clone, Copy)]
#[repr(C)]
pub(in crate::buffer::pool) struct NonEmpty {
    slots: num::NonZero<u32>,
    capacity: num::NonZero<u32>,
}

impl NonEmpty {
    pub(in crate::buffer::pool) fn one(capacity: usize) -> Result<Self, pool::LayoutError> {
        let layout = Layout::new(1, capacity)?;
        Ok(Self {
            slots: num::NonZero::<u32>::MIN,
            capacity: layout.capacity,
        })
    }

    pub(in crate::buffer::pool) fn new(capacity: num::NonZero<u32>) -> Self {
        const { Layout::assert_valid::<1, { u32::MAX as usize }>() };
        Self {
            slots: num::NonZero::<u32>::MIN,
            capacity,
        }
    }

    pub(in crate::buffer::pool) const fn into_layout(self) -> Layout {
        Layout {
            slots: self.slots.get(),
            capacity: self.capacity,
        }
    }
}

impl Layout {
    pub(in crate::buffer::pool) fn try_prefix(
        self,
        slots: num::NonZero<u32>,
    ) -> Result<NonEmpty, pool::GrowingError> {
        if slots.get() > self.slots {
            return Err(pool::GrowingError::Warm {
                requested: slots,
                maximum: self.slots,
            });
        }
        Ok(NonEmpty {
            slots,
            capacity: self.capacity,
        })
    }

    pub(in crate::buffer::pool) fn next(self, used: usize) -> Option<NonEmpty> {
        if used >= (self.slots() as usize) {
            return None;
        }
        let left = (self.slots() as usize) - used;
        let slots = used.max(1).min(left) as u32;
        let slots = num::NonZero::<u32>::MIN.saturating_add(slots - 1);
        Some(NonEmpty {
            slots,
            capacity: self.capacity,
        })
    }
}

const _: () = {
    assert!(size_of::<NonEmpty>() == size_of::<u64>());
};
