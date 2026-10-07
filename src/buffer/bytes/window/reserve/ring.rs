use crate::{alloc, buffer::bytes::window::reserve};

/// An owned ring whose open state always retains its terminal reservation.
pub struct Ring<const N: usize> {
    pub(in crate::buffer::bytes::window::reserve) ring: reserve::Heap,
    pub(in crate::buffer::bytes::window::reserve) closed: bool,
}

impl<const N: usize> Ring<N> {
    pub fn try_new(plan: reserve::Plan<N>) -> Result<Self, alloc::Exhausted> {
        Ok(Self {
            ring: reserve::Heap::try_new(plan.capacity)?,
            closed: false,
        })
    }

    pub fn capacity(&self) -> usize {
        self.ring.capacity() - N
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        let ring = &self.ring;
        ring.as_slices(ring.len())
    }

    /// Publishes the reserved terminal bytes once, without capacity validation.
    pub fn finish(&mut self, source: &[u8; N]) {
        if !self.closed {
            self.ring.append(source);
            self.closed = true;
        }
    }
}

const _: () = {
    assert!(size_of::<Ring<17>>() == 5 * size_of::<usize>());
    assert!(size_of::<Option<Ring<17>>>() == size_of::<Ring<17>>());
};
