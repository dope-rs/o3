use std::marker;

use crate::depot::capacity;

/// One invariant brand for a fixed index domain.
/// As a proof predicate, the retained key lies within this exact domain's fixed count; no occupancy is claimed.
#[derive(Clone, Copy)]
pub struct Domain<'a> {
    count: capacity::Count,
    brand: marker::PhantomData<fn(&'a ()) -> &'a ()>,
}

impl Domain<'_> {
    pub(in crate::depot) fn with<R>(
        count: capacity::Count,
        run: impl for<'a> FnOnce(Domain<'a>) -> R,
    ) -> R {
        run(Domain {
            count,
            brand: marker::PhantomData,
        })
    }

    pub const fn count(self) -> capacity::Count {
        self.count
    }
}

impl crate::depot::Predicate for Domain<'_> {}
