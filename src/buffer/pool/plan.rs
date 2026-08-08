use crate::buffer::pool;

#[derive(Clone, Copy, Debug)]
pub struct Plan {
    max_slots: usize,
    capacity: usize,
}

impl Plan {
    pub fn new(max_slots: usize, capacity: usize) -> Result<Self, pool::LayoutError> {
        pool::Layout::new(max_slots, capacity)?;
        Ok(Self {
            max_slots,
            capacity,
        })
    }

    #[must_use]
    pub fn fixed<const MAX_SLOTS: usize, const CAPACITY: usize>() -> Self {
        let _ = pool::Layout::fixed::<MAX_SLOTS, CAPACITY>();
        Self {
            max_slots: MAX_SLOTS,
            capacity: CAPACITY,
        }
    }

    pub fn layout_up_to(self, requested: usize) -> pool::Layout {
        let slots = requested.min(self.max_slots);
        // SAFETY: the maximum layout was validated and reducing slots cannot overflow it.
        unsafe { pool::Layout::new(slots, self.capacity).unwrap_unchecked() }
    }

    pub const fn max_slots(self) -> usize {
        self.max_slots
    }
}
