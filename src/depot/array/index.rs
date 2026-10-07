use std::{mem, num, ptr};

use crate::depot::array;

/// One position in a fixed array domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Index<const N: usize>(num::NonZero<u8>);

impl<const N: usize> Index<N> {
    /// Admits one position and retains its fixed array bound.
    pub const fn new(index: usize) -> Option<Self> {
        const { assert!(N <= u8::MAX as usize) };
        if index >= N {
            return None;
        }
        Some(Self(num::NonZero::<u8>::MIN.saturating_add(index as u8)))
    }

    /// Selects the low bits of a power-of-two fixed domain.
    pub const fn mask(value: u8) -> Self {
        const { assert!(N <= u8::MAX as usize && N.is_power_of_two()) };
        Self(num::NonZero::<u8>::MIN.saturating_add(value & (N - 1) as u8))
    }

    pub fn from_bit(index: impl crate::depot::raw::BitIndex<N>) -> Self {
        const { assert!(N <= u8::MAX as usize) };
        let index = index.index();
        // SAFETY: BitIndex retains a position below N, whose one-based value fits u8.
        Self(unsafe { num::NonZero::new_unchecked((index as u8).wrapping_add(1)) })
    }

    /// Visits the fixed domain without discarding each position's bound.
    pub fn iter() -> impl DoubleEndedIterator<Item = Self> + ExactSizeIterator {
        const { assert!(N <= u8::MAX as usize) };
        (0..N).map(|index| {
            // SAFETY: iteration admits exactly 0..N and its one-based values fit u8.
            Self(unsafe { num::NonZero::new_unchecked(index as u8 + 1) })
        })
    }

    pub const fn at<const I: usize>() -> Self {
        const { assert!(N <= u8::MAX as usize) };
        const { assert!(I < N) };
        Self(num::NonZero::<u8>::MIN.saturating_add(I as u8))
    }

    /// Searches the prefix ending at this gap and retains the resulting gap.
    pub fn partition<T, const M: usize>(
        self,
        values: &[T; M],
        test: impl FnMut(&T) -> bool,
    ) -> Self {
        const { assert!(M < N && M + 1 == N && N <= u8::MAX as usize) };
        // SAFETY: this retained gap is at most M, including the final endpoint.
        let prefix = unsafe { values.get_unchecked(..self.index()) };
        let index = prefix.partition_point(test);
        // SAFETY: the search returns 0..=prefix.len(), which is at most M < N.
        Self(unsafe { num::NonZero::new_unchecked(index as u8 + 1) })
    }

    /// Embeds this position in a larger fixed domain without another check.
    pub const fn widen<const M: usize>(self) -> Index<M> {
        const { assert!(N <= M && M <= u8::MAX as usize) };
        Index(self.0)
    }

    /// Moves to the preceding position while retaining the narrower domain.
    pub fn previous<const M: usize>(self) -> Option<Index<M>> {
        const { assert!(M < N && M + 1 == N && N <= u8::MAX as usize) };
        num::NonZero::new(self.0.get() - 1).map(Index)
    }

    /// Advances one position into a wider domain that admits the successor.
    /// Stored values in `1..=N` become `2..=N + 1`, within the vacancy domain `1..=M`.
    pub fn next<const M: usize>(self) -> Index<M> {
        const { assert!(N < M && M <= u8::MAX as usize) };
        let slot =
            array::Vacancy::<M>(num::NonZero::<u32>::MIN.saturating_add(self.0.get() as u32));
        Index::from(slot)
    }

    /// Inserts at this exact gap while initializing the complete larger array.
    pub fn insert<'a, T: Copy, const M: usize>(
        self,
        values: &[T; M],
        value: T,
        out: &'a mut mem::MaybeUninit<[T; N]>,
    ) -> &'a mut [T; N] {
        const { assert!(M < N && M + 1 == N) };
        let at = self.index();
        // SAFETY: this gap lies in 0..N = 0..=M. The source prefix, inserted
        // value, and source suffix initialize all N disjoint output positions.
        // The source and exclusive output loan cannot overlap.
        unsafe {
            let target = out.as_mut_ptr().cast::<T>();
            ptr::copy_nonoverlapping(values.as_ptr(), target, at);
            target.add(at).write(value);
            ptr::copy_nonoverlapping(values.as_ptr().add(at), target.add(at + 1), M - at);
            out.assume_init_mut()
        }
    }

    pub const fn index(self) -> usize {
        self.0.get() as usize - 1
    }

    pub fn get<T>(self, values: &[T; N]) -> &T {
        // SAFETY: every constructor retains a position strictly below N.
        unsafe { values.get_unchecked(self.index()) }
    }

    pub fn get_mut<T>(self, values: &mut [T; N]) -> &mut T {
        // SAFETY: the retained bound selects one exclusively borrowed element.
        unsafe { values.get_unchecked_mut(self.index()) }
    }
}

impl<const N: usize> From<array::Vacancy<N>> for Index<N> {
    fn from(slot: array::Vacancy<N>) -> Self {
        const { assert!(N <= u8::MAX as usize) };
        // SAFETY: Vacancy retains a one-based value in 1..=N, and N fits u8.
        Self(unsafe { num::NonZero::new_unchecked(slot.0.get() as u8) })
    }
}

const _: () = {
    assert!(mem::size_of::<Index<1>>() == 1);
    assert!(mem::size_of::<Option<Index<1>>>() == 1);
};
