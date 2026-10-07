use std::num::NonZero;

/// A one-based position bounded by its array extent.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::depot::queue::ring::inline) struct Index<const N: usize>(NonZero<usize>);

impl<const N: usize> Index<N> {
    pub(in crate::depot::queue::ring::inline) const fn bounds() -> Option<(Self, Self)> {
        match NonZero::new(N) {
            Some(value) => Some((Self(NonZero::<usize>::MIN), Self(value))),
            None => None,
        }
    }

    pub(in crate::depot::queue::ring::inline) const fn next(self) -> Option<Self> {
        if self.0.get() == N {
            None
        } else {
            Some(Self(self.0.saturating_add(1)))
        }
    }

    pub(in crate::depot::queue::ring::inline) fn get<T>(self, entries: &[T; N]) -> &T {
        // SAFETY: bounds and next preserve 1..=N in this private type.
        unsafe { entries.get_unchecked(self.0.get() - 1) }
    }
}
