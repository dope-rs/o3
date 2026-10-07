use crate::buffer::pool;

/// A pool layout with a dynamic slot count and static per-slot capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Width<const CAPACITY: usize>(pub(super) u32);

impl<const CAPACITY: usize> Width<CAPACITY> {
    const VALID: () = pool::Layout::assert_valid::<0, CAPACITY>();

    pub fn new(slots: usize) -> Result<Self, pool::LayoutError> {
        let () = Self::VALID;
        let layout = pool::Layout::new(slots, CAPACITY)?;
        Ok(Self(layout.slots()))
    }
}

const _: () = assert!(size_of::<Width<1>>() == size_of::<u32>());
