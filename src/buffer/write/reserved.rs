use std::collections::TryReserveError;

use crate::buffer::{self, write, write::consume};

/// An empty vector suffix whose capacity remains admitted until staging begins.
#[derive(Debug)]
#[must_use = "consume the reservation to stage its initialized suffix"]
pub struct Reserved<'a, L = usize> {
    out: &'a mut Vec<u8>,
    capacity: L,
}

impl<'a, L: buffer::raw::Length> Reserved<'a, L> {
    pub fn try_grow(out: &'a mut Vec<u8>, limit: L) -> Result<Self, crate::alloc::Error> {
        crate::alloc::VecExt::try_grow(out, limit.into_usize())?;
        Ok(Self {
            out,
            capacity: limit,
        })
    }

    pub fn try_reserve(out: &'a mut Vec<u8>, limit: L) -> Result<Self, crate::alloc::Error> {
        crate::alloc::VecExt::try_reserve(out, limit.into_usize())?;
        Ok(Self {
            out,
            capacity: limit,
        })
    }

    pub fn try_new(out: &'a mut Vec<u8>, limit: L) -> Result<Self, consume::Error> {
        consume::Error::check(limit.into_usize(), out.capacity() - out.len())?;
        Ok(Self {
            out,
            capacity: limit,
        })
    }

    /// Transfers the same exclusive suffix without repeating capacity admission.
    pub fn into_staged(self) -> write::Staged<'a, L> {
        write::Staged {
            out: self.out,
            len: L::ZERO,
            capacity: self.capacity,
        }
    }
}

impl<'a> Reserved<'a> {
    /// Preserves the standard vector's exact reservation error and admitted owner.
    pub fn try_exact(out: &'a mut Vec<u8>, limit: usize) -> Result<Self, TryReserveError> {
        out.try_reserve_exact(limit)?;
        Ok(Self {
            out,
            capacity: limit,
        })
    }
}

impl<'a> From<&'a mut Vec<u8>> for Reserved<'a> {
    /// Retains the vector's entire available suffix under its exclusive borrow.
    fn from(out: &'a mut Vec<u8>) -> Self {
        Self {
            capacity: out.capacity() - out.len(),
            out,
        }
    }
}

const _: () = {
    assert!(size_of::<Reserved<'static>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Reserved<'static>>>() == size_of::<Reserved<'static>>());
    assert!(!std::mem::needs_drop::<Reserved<'static>>());
};
