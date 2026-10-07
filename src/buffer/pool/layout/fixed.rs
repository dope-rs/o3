use crate::buffer::pool;

/// A pool layout proven entirely from compile-time dimensions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Fixed<const SLOTS: usize, const CAPACITY: usize>(pub(super) ());

impl<const SLOTS: usize, const CAPACITY: usize> Fixed<SLOTS, CAPACITY> {
    const VALID: () = pool::Layout::assert_valid::<SLOTS, CAPACITY>();

    #[must_use]
    pub const fn new() -> Self {
        let () = Self::VALID;
        Self(())
    }
}

const _: () = assert!(size_of::<Fixed<1, 1>>() == 0);

impl<const SLOTS: usize, const CAPACITY: usize> Default for Fixed<SLOTS, CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
