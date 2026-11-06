use crate::{
    buffer,
    buffer::{pool, pool::owners, write::consume::Error},
};

/// One measured source bound to the lease that admitted its entire write.
#[must_use = "commit consumes the source and publishes its bytes"]
pub struct Copy<'a, const N: usize> {
    source: buffer::raw::Slices<'a, N>,
    target: pool::Span,
}

impl<'a, const N: usize> Copy<'a, N> {
    pub fn try_new(source: buffer::raw::Slices<'a, N>, target: pool::Span) -> Result<Self, Error> {
        Error::check(
            source.len().get() as usize,
            target.capacity().get() as usize - target.len(),
        )?;
        Ok(Self { source, target })
    }

    /// Initializes and freezes the retained lease without checking bounds again.
    pub fn commit(mut self) -> pool::Span<owners::Plain, pool::Owned, pool::Frozen> {
        use buffer::raw::SliceExt as _;

        let target = self.target.entry().data();
        let start = self.target.len();
        // SAFETY: Copy retains every measured source and its admitted disjoint lease suffix.
        unsafe { self.source.slices().copy_to(target.add(start)) };
        self.target.len = start as u32 + self.source.len().get();
        self.target.freeze()
    }
}

const _: () = {
    assert!(size_of::<Copy<'static, 1>>() == 5 * size_of::<usize>());
    assert!(size_of::<Option<Copy<'static, 1>>>() == size_of::<Copy<'static, 1>>());
    assert!(size_of::<Copy<'static, 3>>() == 9 * size_of::<usize>());
    assert!(size_of::<Option<Copy<'static, 3>>>() == size_of::<Copy<'static, 3>>());
};
