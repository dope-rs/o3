use std::num::NonZero;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::depot::queue::group::map) struct Mask(NonZero<u64>);

impl Mask {
    pub(in crate::depot::queue::group::map) fn between(key: u64, existing: u64) -> Option<Self> {
        Some(Self(NonZero::new(key ^ existing)?.isolate_highest_one()))
    }

    pub(in crate::depot::queue::group::map) fn contains(self, key: u64) -> bool {
        key & self.0.get() != 0
    }
}
