use crate::depot::capacity;

/// The original capacity exhausted before an insertion begins.
#[derive(Debug)]
#[repr(transparent)]
pub struct Full<C = capacity::Count> {
    pub capacity: C,
}

const _: () = {
    assert!(size_of::<Full>() == size_of::<capacity::Count>());
    assert!(size_of::<Full<std::num::NonZero<u32>>>() == size_of::<std::num::NonZero<u32>>());
    assert!(
        size_of::<Option<Full<std::num::NonZero<u32>>>>() == size_of::<std::num::NonZero<u32>>()
    );
};
