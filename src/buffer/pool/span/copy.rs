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
    pub fn commit(self) -> pool::Span<owners::Plain, pool::Owned, pool::Frozen> {
        buffer::raw::Copy::copy(self)
    }
}

// SAFETY: Copy retains every measured source and its admitted exclusive lease suffix.
unsafe impl<const N: usize> buffer::raw::Copy for Copy<'_, N> {
    type Output = pool::Span<owners::Plain, pool::Owned, pool::Frozen>;

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (
            *self.source.slices(),
            self.target.entry().data(),
            self.target.len(),
        )
    }

    unsafe fn publish(mut self) -> Self::Output {
        self.target.len += self.source.len().get();
        self.target.freeze()
    }
}

const _: () = {
    assert!(size_of::<Copy<'static, 1>>() == 5 * size_of::<usize>());
    assert!(size_of::<Option<Copy<'static, 1>>>() == size_of::<Copy<'static, 1>>());
    assert!(size_of::<Copy<'static, 3>>() == 9 * size_of::<usize>());
    assert!(size_of::<Option<Copy<'static, 3>>>() == size_of::<Copy<'static, 3>>());
};
