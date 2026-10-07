use crate::depot::queue::links;

/// One index from an exact slot list, or its distinct accounting table.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::buffer::pool) struct Key<const ACCOUNT: bool = false>(u32);

impl<const ACCOUNT: bool> Key<ACCOUNT> {
    /// # Safety
    /// The index belongs to this exact initialized pool's Accounts table when
    /// ACCOUNT is true, and to its Slot list otherwise.
    pub(super) const unsafe fn new(index: u32) -> Self {
        // SAFETY: both initialized arrays exclude the compact sentinel.
        Self(unsafe { links::Index::new_raw(index) }.raw())
    }

    pub(super) const fn get(self) -> u32 {
        self.0
    }

    /// # Safety
    /// Every non-sentinel word belongs to the exact initialized table selected
    /// by ACCOUNT; the caller preserves the word's vacant or published state.
    pub(super) const unsafe fn decode(word: u32) -> Option<Self> {
        match links::Index::decode(word) {
            Some(index) => Some(Self(index.raw())),
            None => None,
        }
    }

    pub(super) const fn encode(key: Option<Self>) -> u32 {
        links::Index::encode(match key {
            // SAFETY: each key originates from its exact initialized array,
            // whose admitted length excludes the sentinel index.
            Some(key) => Some(unsafe { links::Index::new_raw(key.0) }),
            None => None,
        })
    }
}

const _: () = {
    assert!(size_of::<Key>() == size_of::<u32>());
    assert!(size_of::<Key<true>>() == size_of::<u32>());
};
