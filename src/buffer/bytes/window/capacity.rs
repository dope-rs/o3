use std::num;

use crate::buffer::write::consume;

/// One nonzero window capacity bounded by its owning window's ceiling.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::buffer) struct Capacity<const MAX: usize>(num::NonZero<u32>);

impl<const MAX: usize> Capacity<MAX> {
    const VALID: () = assert!(
        MAX <= u32::MAX as usize,
        "window capacity ceiling must fit u32"
    );

    pub(in crate::buffer) fn try_new(value: num::NonZero<usize>) -> Result<Self, consume::Error> {
        let () = Self::VALID;
        consume::Error::check(value.get(), MAX)?;
        let value = num::NonZero::<u32>::MIN.saturating_add(value.get() as u32 - 1);
        Ok(Self(value))
    }

    pub(super) fn try_append<const N: usize>(
        snapshot: &super::Snapshot<MAX>,
        first: usize,
        slices: &[&[u8]; N],
    ) -> Result<Option<Self>, consume::Error> {
        let () = Self::VALID;
        let used = snapshot.len();
        let added = consume::Error::checked_end(first, MAX - used, slices)?;
        Ok(num::NonZero::new(added).map(|added| {
            // SAFETY: added is positive and checked_end bounds the sum by MAX, which fits u32.
            Self(unsafe { crate::mem::raw::Counter::nonzero((used + added.get()) as u32) })
        }))
    }

    pub(in crate::buffer) const fn get(self) -> u32 {
        self.0.get()
    }

    pub(in crate::buffer) const fn nonzero(self) -> num::NonZero<u32> {
        self.0
    }

    pub(in crate::buffer::bytes::window) fn grow(self, required: Self) -> Self {
        let ceiling = Self(num::NonZero::<u32>::MIN.saturating_add(MAX as u32 - 1));
        self.grow_to(required, ceiling)
    }

    /// Doubles this capacity within the owner's admitted ceiling.
    /// Both current and required capacities belong to that unchanged ceiling.
    pub(in crate::buffer) fn grow_to(self, required: Self, ceiling: Self) -> Self {
        let mut target = self.0;
        while target < required.0 {
            target = target.saturating_add(target.get()).min(ceiling.0);
        }
        Self(target)
    }
}

impl From<num::NonZero<u32>> for Capacity<{ u32::MAX as usize }> {
    fn from(value: num::NonZero<u32>) -> Self {
        Self(value)
    }
}

const _: () = {
    assert!(size_of::<Capacity<1>>() == size_of::<u32>());
    assert!(size_of::<Option<Capacity<1>>>() == size_of::<u32>());
};
