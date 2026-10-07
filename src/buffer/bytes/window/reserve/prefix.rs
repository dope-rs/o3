use crate::buffer::bytes::window::reserve;

/// An unchanged, bounded prefix of one exclusively borrowed ring.
#[must_use = "commit consumes the selected prefix; dropping it leaves the ring unchanged"]
pub struct Prefix<'a, const N: usize> {
    ring: &'a mut reserve::Ring<N>,
    len: usize,
}

impl<'a, const N: usize> Prefix<'a, N> {
    pub fn new(ring: &'a mut reserve::Ring<N>, max: usize) -> Self {
        let len = ring.len().min(max);
        Self { ring, len }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub fn total(&self) -> usize {
        self.ring.len()
    }

    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        let core = &self.ring.ring;
        core.as_slices(self.len)
    }

    pub fn truncate(&mut self, max: usize) {
        self.len = self.len.min(max);
    }

    pub fn commit(self) -> &'a mut reserve::Ring<N> {
        let core = &mut self.ring.ring;
        core.consume(self.len);
        self.ring
    }
}

const _: () = {
    assert!(size_of::<Prefix<'static, 17>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Prefix<'static, 17>>>() == size_of::<Prefix<'static, 17>>());
};
