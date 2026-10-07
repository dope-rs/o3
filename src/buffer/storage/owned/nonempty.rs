use std::{num::NonZero, ops};

use crate::{
    alloc,
    buffer::{
        self,
        write::{Append, Sink as _},
    },
    mem::Rejected,
};

/// Zero-length byte extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zero;

/// One byte owner whose exposed mutations preserve a nonempty initialized extent.
#[derive(Debug)]
#[repr(transparent)]
pub struct NonEmpty<T>(T);

impl<T: buffer::raw::Source> NonEmpty<T> {
    pub fn try_new(value: T) -> Result<Self, Rejected<T, Zero>> {
        if value.as_ref().is_empty() {
            return Err(Rejected::new(value, Zero));
        }
        Ok(Self(value))
    }
}

impl<T> NonEmpty<T> {
    pub fn into_storage(self) -> T {
        self.0
    }
}

impl NonEmpty<Vec<u8>> {
    /// Appending the first byte establishes the proof on this exact allocation.
    pub fn try_push(mut value: Vec<u8>, byte: u8) -> Result<Self, Rejected<Vec<u8>, alloc::Error>> {
        if let Err(error) = value.write_byte(byte) {
            return Err(Rejected::new(value, error));
        }
        Ok(Self(value))
    }

    pub fn try_resize(
        mut value: Vec<u8>,
        len: NonZero<usize>,
        byte: u8,
    ) -> Result<Self, Rejected<Vec<u8>, alloc::Error>> {
        let added = len.get().saturating_sub(value.len());
        if let Err(error) = Append::new(&mut value).try_fill(added, byte) {
            return Err(Rejected::new(value, error));
        }
        value.truncate(len.get());
        Ok(Self(value))
    }

    pub fn append(&mut self) -> Append<'_> {
        Append::new(&mut self.0)
    }

    pub fn first_mut(&mut self) -> &mut u8 {
        // SAFETY: construction admits a nonempty initialized vector, and every
        // exposed mutation preserves its first byte on this exact owner.
        unsafe { self.0.get_unchecked_mut(0) }
    }

    pub fn truncate(&mut self, len: NonZero<usize>) {
        self.0.truncate(len.get());
    }
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for NonEmpty<T> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<T: buffer::Residency> buffer::Residency for NonEmpty<T> {
    fn resident(&self) -> usize {
        self.0.resident()
    }
}

impl<T: AsRef<[u8]>> ops::Deref for NonEmpty<T> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl ops::DerefMut for NonEmpty<Vec<u8>> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut_slice()
    }
}

impl<T: AsRef<[u8]>> NonEmpty<T> {
    pub fn len(&self) -> NonZero<usize> {
        // SAFETY: admission requires a stable source, and every exposed mutation
        // preserves at least one byte on this exact owner.
        unsafe { NonZero::new_unchecked(self.0.as_ref().len()) }
    }
}

// SAFETY: the wrapper preserves its immutable source extent through shared access.
unsafe impl<T: buffer::raw::Source> buffer::raw::Source for NonEmpty<T> {}

// SAFETY: consuming the nonempty proof returns the exact original owner unchanged.
unsafe impl<T> buffer::raw::Convert<T> for NonEmpty<T> {
    fn into(self) -> T {
        self.0
    }
}

const _: () = {
    assert!(size_of::<NonEmpty<Vec<u8>>>() == size_of::<Vec<u8>>());
    assert!(size_of::<Option<NonEmpty<Vec<u8>>>>() == size_of::<Option<Vec<u8>>>());
};
