/// A statically checked plane in one declared domain.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Id<const N: usize> {
    index: u32,
}

impl<const N: usize> Id<N> {
    pub const fn at<const P: usize>() -> Self {
        const {
            assert!(P < N, "plane must be in its declared domain");
            assert!(P <= u32::MAX as usize, "plane must fit its stored width");
        }
        Self { index: P as u32 }
    }

    pub(in crate::depot::index::set) const fn offset(self) -> usize {
        self.index as usize
    }
}
