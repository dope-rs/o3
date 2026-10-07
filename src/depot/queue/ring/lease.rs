use crate::depot::queue::ring;

/// One reservation, consumed by publication or explicit capacity release.
#[must_use = "publish the reserved value or release its capacity"]
pub struct Lease<'a, T> {
    pub(in crate::depot::queue::ring) pool: &'a ring::Pool<T>,
}

impl<T> Lease<'_, T> {
    pub fn push_back(self, value: T) {
        let Self { pool } = self;
        // SAFETY: this affine lease excludes one slot from every other reservation.
        unsafe { crate::depot::raw::RingCell::insert::<false>(pool, value) };
    }

    pub fn push_front(self, value: T) {
        let Self { pool } = self;
        // SAFETY: this affine lease excludes one slot from every other reservation.
        unsafe { crate::depot::raw::RingCell::insert::<true>(pool, value) };
    }

    pub fn release(self) {
        let Self { pool } = self;
        let free = pool.block.state();
        free.set(free.get() + 1);
    }
}

const _: () = {
    assert!(size_of::<Lease<'static, u64>>() == size_of::<usize>());
    assert!(size_of::<Option<Lease<'static, u64>>>() == size_of::<Lease<'static, u64>>());
};
