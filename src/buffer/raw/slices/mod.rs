use std::{num::NonZero, ptr};

use resident::copy;
use storage::{alloc, regions::Initialize as _};

use crate::buffer::{self, bytes::bounded, resident, storage, write::consume};

/// Borrowed slices whose byte sum is exactly their nonzero allocation capacity.
#[must_use]
pub struct Slices<'a, const N: usize> {
    capacity: NonZero<u32>,
    values: [&'a [u8]; N],
}

impl<'a, const N: usize> Slices<'a, N> {
    /// # Safety
    /// `capacity` must equal the sum of every slice length.
    pub const unsafe fn new(capacity: NonZero<u32>, values: [&'a [u8]; N]) -> Self {
        Self { capacity, values }
    }

    pub const fn len(&self) -> NonZero<u32> {
        self.capacity
    }

    pub(in crate::buffer) fn slices(&self) -> &[&'a [u8]; N] {
        &self.values
    }
}

const _: () = {
    assert!(size_of::<Slices<'static, 1>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Slices<'static, 1>>>() == size_of::<Slices<'static, 1>>());
    assert!(size_of::<Slices<'static, 3>>() == 7 * size_of::<usize>());
    assert!(size_of::<Option<Slices<'static, 3>>>() == size_of::<Slices<'static, 3>>());
};

pub(in crate::buffer) trait SliceExt {
    /// # Safety
    /// `target` must retain exclusive writable storage for the complete byte sum,
    /// which must fit `usize`. Every source must be disjoint from that storage.
    unsafe fn copy_to(&self, target: *mut u8);
}

impl<T: SliceExt + ?Sized> SliceExt for &T {
    unsafe fn copy_to(&self, target: *mut u8) {
        // SAFETY: borrowing preserves the original sources and their complete extent.
        unsafe { T::copy_to(self, target) };
    }
}

impl SliceExt for [u8] {
    unsafe fn copy_to(&self, target: *mut u8) {
        // SAFETY: the caller retains this source and its complete disjoint target.
        unsafe { ptr::copy_nonoverlapping(self.as_ptr(), target, self.len()) };
    }
}

impl<const N: usize> SliceExt for [&[u8]; N] {
    unsafe fn copy_to(&self, target: *mut u8) {
        // SAFETY: the caller retains every slice and its complete disjoint target.
        unsafe { buffer::raw::copy_slices(self.iter().copied(), target) };
    }
}

impl<I: SliceExt> SliceExt for (&[u8], I) {
    unsafe fn copy_to(&self, target: *mut u8) {
        // SAFETY: admission covers the prefix and remaining sources in order.
        unsafe {
            self.0.copy_to(target);
            self.1.copy_to(target.add(self.0.len()));
        }
    }
}

impl<const P: usize, const N: usize> SliceExt for ([u8; P], [&[u8]; N]) {
    unsafe fn copy_to(&self, target: *mut u8) {
        // SAFETY: admission includes the complete fixed prefix and every trailing source.
        unsafe { (self.0.as_slice(), &self.1).copy_to(target) };
    }
}

mod proofs;
pub(in crate::buffer) use proofs::{
    Batch, Copy, Exclusive, Indexed, IndexedMut, Linked, LinkedMut, Prefix,
};

impl<'a, const N: usize> Slices<'a, N> {
    /// Admits one exact slice set whose byte sum fits the allocation domain.
    pub fn try_new(values: [&'a [u8]; N]) -> Result<Option<Self>, consume::Error> {
        let capacity = consume::Error::checked_end(0, u32::MAX as usize, &values)?;
        Ok(NonZero::<u32>::new(capacity as u32).map(|capacity| {
            // SAFETY: admission proves `capacity` is the sum of every slice length.
            unsafe { Self::new(capacity, values) }
        }))
    }
}

impl<const N: usize> Slices<'_, N> {
    /// Allocates and initializes the exact admitted source without revalidation.
    pub fn try_owned(self) -> Result<storage::Exact, crate::alloc::Exhausted> {
        let inner = self.try_init(()).map_err(alloc::Error::into_alloc)?;
        Ok(storage::Exact { inner: Some(inner) })
    }
}

impl<'a> Slices<'a, 1> {
    /// Admits one complete bounded source as an exact nonzero allocation.
    pub(in crate::buffer) fn from_source(source: &'a impl bounded::Source) -> Option<Self> {
        let bytes = source.as_ref();
        NonZero::new(bytes.len() as u32).map(|capacity| {
            // SAFETY: the single retained slice is exactly `capacity` bytes.
            unsafe { Self::new(capacity, [bytes]) }
        })
    }
}

impl<const N: usize> Slices<'_, N> {
    /// Copies the admitted source into one fresh allocation charged to `budget`.
    pub fn try_copy<'d>(
        self,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Exact<'d>, copy::Error> {
        let init = self.try_init::<resident::Lease<'d>>(*budget)?;
        Ok(resident::Exact::from_init(init))
    }
}

// SAFETY: shared access preserves the slice's exact range and contents.
unsafe impl buffer::raw::Source for [u8] {}

// SAFETY: shared access preserves the array's complete range and contents.
unsafe impl<const N: usize> buffer::raw::Source for [u8; N] {}

// SAFETY: the immutable borrow preserves the exact slice and its contents.
unsafe impl buffer::raw::Source for &[u8] {}

// SAFETY: the immutable borrow preserves the exact array and its contents.
unsafe impl<const N: usize> buffer::raw::Source for &[u8; N] {}

// SAFETY: shared access cannot mutate Vec; its Sink implementation only appends.
unsafe impl buffer::raw::Source for Vec<u8> {}

// SAFETY: Vec's byte projection returns its existing initialized range unchanged.
unsafe impl buffer::raw::Mutable for Vec<u8> {}

// SAFETY: reborrowing this exclusive slice preserves its descriptor and contents.
unsafe impl buffer::raw::Mutable for &mut [u8] {}
