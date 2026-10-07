use std::num;

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(transparent)]
pub struct Direct(u32);

impl Direct {
    /// # Safety
    /// `index` must belong to the checked domain of the receiving heap, which
    /// excludes `u32::MAX`.
    pub(in crate::depot) const unsafe fn from_index(index: usize) -> Self {
        Self(index as u32)
    }

    pub(in crate::depot) const fn get(self) -> usize {
        self.0 as usize
    }

    pub(in crate::depot) const fn position(self) -> num::NonZero<u32> {
        // SAFETY: the admitted direct index stays below u32::MAX, so the increment cannot wrap.
        unsafe { num::NonZero::new_unchecked(self.0 + 1) }
    }
}

const _: () = assert!(std::mem::size_of::<Direct>() == std::mem::size_of::<u32>());

// SAFETY: each immutable value preserves its exact direct-address position.
unsafe impl crate::depot::raw::Address for Direct {
    fn index(self) -> usize {
        self.0 as usize
    }
}
