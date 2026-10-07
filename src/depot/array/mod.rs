//! Fixed-capacity arrays whose initialized length is carried by [`Len`].

use std::{mem, num, ops, ptr};

use prefix::Prefix;

use crate::mem::{Lease, lease};

pub mod bits;
mod compact;
pub mod copy;
mod index;
pub use index::Index;
pub mod init;
mod prefix;
pub mod sorted;

/// A length proven to fit the fixed capacity `N`.
///
/// A zero-capacity array cannot manufacture a nonzero length.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Len<const N: usize>(pub(in crate::depot::array) u32);

/// An observed length beyond the fixed capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub actual: usize,
    pub capacity: usize,
}

/// Affine proof that one exact position remains after a fixed-array length.
#[must_use = "a fixed-array vacancy must be committed or abandoned"]
#[repr(transparent)]
#[derive(Debug)]
pub struct Vacancy<const N: usize>(num::NonZero<u32>);

impl<const N: usize> Len<N> {
    pub(in crate::depot::array) const VALID: () =
        assert!(N <= u32::MAX as usize, "inline array capacity must fit u32");

    pub const ZERO: Self = {
        let () = Self::VALID;
        Self(0)
    };

    pub const ONE: Self = {
        let () = Self::VALID;
        assert!(N != 0, "zero-capacity array has no length one");
        Self(1)
    };

    /// Constructs the length from a compile-time value within the capacity.
    pub const fn exact<const LENGTH: usize>() -> Self {
        let () = Self::VALID;
        const { assert!(LENGTH <= N, "capacity") };
        Self(LENGTH as u32)
    }

    pub const MAX: Self = {
        let () = Self::VALID;
        Self(N as u32)
    };

    pub const fn get(self) -> usize {
        self.0 as usize
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn is_full(self) -> bool {
        self.0 == N as u32
    }

    /// Returns the exact next position and the length after occupying it.
    pub const fn vacancy(self) -> Option<Vacancy<N>> {
        if self.0 >= N as u32 {
            return None;
        }
        // SAFETY: the non-full bounded length is below u32::MAX.
        Some(Vacancy(unsafe { num::NonZero::new_unchecked(self.0 + 1) }))
    }

    /// Borrows the prefix bounded by this retained length.
    pub fn prefix<T>(self, values: &[T; N]) -> &[T] {
        // SAFETY: every Len constructor proves this length is at most N.
        unsafe { values.get_unchecked(..self.get()) }
    }

    /// Borrows the mutable prefix bounded by this retained length.
    pub fn prefix_mut<T>(self, values: &mut [T; N]) -> &mut [T] {
        // SAFETY: every Len constructor proves this length is at most N.
        unsafe { values.get_unchecked_mut(..self.get()) }
    }
}

impl<const N: usize> TryFrom<usize> for Len<N> {
    type Error = Error;

    fn try_from(len: usize) -> Result<Self, Self::Error> {
        let () = Self::VALID;
        if len > N {
            return Err(Error {
                actual: len,
                capacity: N,
            });
        }
        Ok(Self(len as u32))
    }
}

impl<const N: usize> Vacancy<N> {
    /// Admits one compile-time position within the fixed array domain.
    pub const fn at<const I: usize>() -> Self {
        let () = Len::<N>::VALID;
        const { assert!(I < N) };
        Self(num::NonZero::<u32>::MIN.saturating_add(I as u32))
    }

    /// Returns the exact position owned by this vacancy.
    pub const fn index(&self) -> usize {
        (self.0.get() - 1) as usize
    }

    /// Borrows the same admitted position in a complete array.
    pub fn get_mut<'a, T>(&self, values: &'a mut [T; N]) -> &'a mut T {
        // SAFETY: this vacancy's position is strictly below its retained capacity N.
        unsafe { values.get_unchecked_mut(self.index()) }
    }

    /// Commits this vacancy and returns the resulting initialized length.
    pub const fn commit(self) -> Len<N> {
        Len(self.0.get())
    }
}

const _: () = {
    assert!(mem::size_of::<Vacancy<4>>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Vacancy<4>>>() == mem::size_of::<u32>());
};

impl<const N: usize> From<Len<N>> for usize {
    fn from(len: Len<N>) -> Self {
        len.get()
    }
}

/// Exclusive proof for the exact next position in one inline vector.
#[must_use = "a vector vacancy must be filled or abandoned"]
pub struct Vacant<'a, T, const N: usize> {
    raw: &'a mut Prefix<T, N>,
    slot: Vacancy<N>,
}

impl<T, const N: usize> Vacant<'_, T, N> {
    /// Inspects the initialized prefix while this vacancy remains borrowed.
    pub fn as_slice(&self) -> &[T] {
        self.raw.as_slice()
    }

    /// Fills this exact vacancy and commits the vector's next length.
    pub fn insert(self, value: T) {
        self.slot.get_mut(&mut self.raw.entries).write(value);
        self.raw.len = self.slot.commit();
    }
}

/// An inline vector with a fixed capacity and an owned initialized prefix.
#[repr(transparent)]
pub struct Vec<T, const N: usize> {
    raw: Prefix<T, N>,
}

/// A consuming iterator that owns the unread suffix of an [`Vec`].
#[derive(Debug)]
#[repr(transparent)]
pub struct IntoIter<T, const N: usize> {
    raw: copy::IntoIter<T, N>,
}

/// Sole return entitlement for the iterator's most recently removed value.
/// Dropping this entitlement leaves the iterator advanced.
pub struct Withdrawn<'a, T, const N: usize> {
    source: &'a mut IntoIter<T, N>,
}

impl<T, const N: usize> Withdrawn<'_, T, N> {
    /// Returns the original position retained by this exclusive loan.
    pub fn index(&self) -> usize {
        self.source.raw.front.get().wrapping_sub(1)
    }
}

impl<T, const N: usize> crate::mem::Return for Withdrawn<'_, T, N> {
    type Item = T;

    fn return_item(self, item: T) {
        let front = Len(self.source.raw.front.0.wrapping_sub(1));
        // SAFETY: removal advanced this exact iterator once, and the exclusive
        // loan retains its original vacant position until restoration.
        unsafe {
            self.source
                .raw
                .entries
                .get_unchecked_mut(front.get())
                .write(item)
        };
        self.source.raw.front = front;
    }
}

impl<T, const N: usize> Vec<T, N> {
    pub const fn new() -> Self {
        Self {
            raw: Prefix::empty(),
        }
    }

    pub fn from_one(value: T) -> Self {
        Self {
            raw: Prefix::single(value),
        }
    }

    pub fn from_fn(len: Len<N>, f: impl FnMut(Vacancy<N>) -> T) -> Self {
        Self {
            raw: Prefix::fill(len, f),
        }
    }

    /// Maps one bounded source into this destination while retaining its borrow.
    pub fn map_from<'a, U: Copy>(
        &mut self,
        source: &'a copy::Vec<U, N>,
        make: impl FnMut(Vacancy<N>, &'a U) -> T,
    ) {
        self.raw.map_from(&source.raw, make);
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        let Some(vacant) = self.vacancy() else {
            return Err(value);
        };
        vacant.insert(value);
        Ok(())
    }

    /// Borrows the exact next position when this vector has capacity.
    pub fn vacancy(&mut self) -> Option<Vacant<'_, T, N>> {
        self.raw.vacancy()
    }

    /// Returns the initialized-prefix length with its fixed capacity attached.
    pub const fn extent(&self) -> Len<N> {
        self.raw.len
    }

    /// Transfers the initialized prefix while retaining its original backing.
    pub fn drain(&mut self) -> init::IntoIter<'_, T> {
        init::IntoIter::drain(&mut self.raw)
    }

    pub fn as_slice(&self) -> &[T] {
        self.raw.as_slice()
    }
}

impl<T, const N: usize> Default for Vec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> From<[T; N]> for Vec<T, N> {
    fn from(values: [T; N]) -> Self {
        Self {
            raw: Prefix::from_array(values),
        }
    }
}

impl<T, const N: usize> ops::Deref for Vec<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.raw.as_slice()
    }
}

impl<T, const N: usize> ops::DerefMut for Vec<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.raw.as_mut_slice()
    }
}

impl<T, const N: usize> IntoIterator for Vec<T, N> {
    type IntoIter = IntoIter<T, N>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        let owner = mem::ManuallyDrop::new(self);
        // SAFETY: this consuming owner transition disables its drop and moves
        // the initialized prefix exactly once into the unread-suffix owner.
        let prefix = unsafe { ptr::read(&owner.raw) };
        IntoIter {
            raw: copy::IntoIter::new(prefix),
        }
    }
}

impl<T, const N: usize> Drop for Vec<T, N> {
    fn drop(&mut self) {
        self.raw.drop_all();
    }
}

impl<T, const N: usize> IntoIter<T, N> {
    /// Removes the next value under its exact return entitlement.
    /// Dropping the lease restores it; disarming the lease retains advancement.
    pub fn remove_front(&mut self) -> Option<Lease<Withdrawn<'_, T, N>, lease::Open>> {
        let value = self.raw.next()?;
        Some(Lease::open(Withdrawn { source: self }, value))
    }
}

impl<T, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.raw.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.raw.size_hint()
    }
}

impl<T, const N: usize> ExactSizeIterator for IntoIter<T, N> {}

impl<T, const N: usize> Drop for IntoIter<T, N> {
    fn drop(&mut self) {
        crate::depot::raw::Drain::destroy::<false>(&mut self.raw);
    }
}
