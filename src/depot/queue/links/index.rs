use std::num::NonZero;

use crate::{base::num::bounded, depot::queue::links};
/// One live compact topology index, excluding the `Index::END` sentinel.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Index(NonZero<u32>);

impl Index {
    pub(in crate::depot) const END: u32 = u32::MAX;

    pub fn try_new(index: usize) -> Option<Self> {
        if index >= Index::END as usize {
            return None;
        }
        Some(unsafe { Self::new_usize(index) })
    }

    pub(crate) const fn decode(raw: u32) -> Option<Self> {
        match NonZero::new(!raw) {
            Some(index) => Some(Self(index)),
            None => None,
        }
    }

    pub(crate) const fn encode(index: Option<Self>) -> u32 {
        match index {
            Some(index) => index.raw(),
            None => Self::END,
        }
    }

    /// # Safety
    /// `index` is below the compact `Index::END` sentinel.
    pub(in crate::depot) const unsafe fn new_usize(index: usize) -> Self {
        unsafe { Self::new_raw(index as u32) }
    }

    /// # Safety
    /// `raw` is not the compact `Index::END` sentinel.
    pub(crate) const unsafe fn new_raw(raw: u32) -> Self {
        Self(unsafe { NonZero::new_unchecked(!raw) })
    }

    pub const fn get(self) -> usize {
        self.raw() as usize
    }

    pub const fn raw(self) -> u32 {
        !self.0.get()
    }

    pub const fn position(self) -> NonZero<u32> {
        self.0.cast_signed().wrapping_neg().cast_unsigned()
    }

    pub(crate) const fn from_pos(position: NonZero<u32>) -> Self {
        Self(position.cast_signed().wrapping_neg().cast_unsigned())
    }

    pub fn is_linked(self, hook: &links::Hook) -> bool {
        hook.previous.get() != self.raw()
    }
}

impl<const MAX: u32> From<bounded::U32<0, MAX>> for Index {
    fn from(index: bounded::U32<0, MAX>) -> Self {
        const { assert!(MAX < u32::MAX) };
        // SAFETY: the bounded index excludes Index::END, so its complement is nonzero.
        unsafe { Self::new_raw(index.get()) }
    }
}

/// Converts a one-based position into its non-sentinel slot index.
impl From<NonZero<u32>> for Index {
    fn from(position: NonZero<u32>) -> Self {
        Self::from_pos(position)
    }
}

const _: () = {
    assert!(std::mem::size_of::<Index>() == std::mem::size_of::<u32>());
    assert!(std::mem::size_of::<Option<Index>>() == std::mem::size_of::<u32>());
};
