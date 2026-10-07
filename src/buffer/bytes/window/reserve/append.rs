use crate::buffer::{bytes::window::reserve, write::consume::Error};

/// Exact source slices admitted while preserving the ring's terminal state.
#[must_use = "call commit to publish the admitted sources"]
pub struct Append<'a, 'source, const N: usize, const S: usize> {
    ring: &'a mut reserve::Ring<N>,
    slices: [&'source [u8]; S],
}

impl<'a, 'source, const N: usize, const S: usize> Append<'a, 'source, N, S> {
    pub fn try_new(
        ring: &'a mut reserve::Ring<N>,
        slices: [&'source [u8]; S],
        limit: usize,
    ) -> Result<Self, Error> {
        let core = &ring.ring;
        let capacity = core.capacity() - if ring.closed { 0 } else { N };
        Error::checked_end(core.len(), capacity.min(limit), &slices)?;
        Ok(Self { ring, slices })
    }

    pub fn commit(self) {
        let Self { ring, slices } = self;
        let core = &mut ring.ring;
        for source in slices {
            core.append(source);
        }
    }
}

const _: () = {
    assert!(size_of::<Append<'static, 'static, 17, 1>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Append<'static, 'static, 17, 1>>>() == 3 * size_of::<usize>());
};
