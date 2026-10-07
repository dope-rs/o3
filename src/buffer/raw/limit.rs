use std::{marker, mem};

use crate::{buffer, buffer::storage::provided::view::LengthError};

/// Resident byte ceiling supplied to one projection operation.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(transparent)]
pub struct Limit<'id> {
    resident: u32,
    operation: marker::PhantomData<fn(&'id mut ()) -> &'id mut ()>,
}

impl<'id> Limit<'id> {
    pub const fn get(self) -> u32 {
        self.resident
    }

    /// Projects the prefix proved by the foreign owner contract.
    /// # Safety
    /// Prove `bytes.len() <= self.get()` while retaining this exact slice and source owner.
    pub unsafe fn project<'a>(self, bytes: &'a mut [u8]) -> buffer::raw::Projection<'a, 'id> {
        buffer::raw::Projection {
            bytes,
            operation: self.operation,
        }
    }
}

impl Limit<'_> {
    pub(in crate::buffer) fn scope<R>(
        resident: u32,
        apply: impl for<'id> FnOnce(Limit<'id>) -> R,
    ) -> R {
        apply(Limit {
            resident,
            operation: marker::PhantomData,
        })
    }
}

const _: () = {
    assert!(mem::size_of::<Limit<'static>>() == mem::size_of::<u32>());
    assert!(
        mem::size_of::<buffer::raw::Projection<'static, 'static>>() == 2 * mem::size_of::<usize>()
    );
};

impl<'id> Limit<'id> {
    /// Projects one mutable item extent after checking its resident ceiling.
    pub fn try_project<'a>(
        self,
        bytes: &'a mut [u8],
    ) -> Result<buffer::raw::Projection<'a, 'id>, LengthError> {
        if bytes.len() > self.get() as usize {
            return Err(LengthError::new(bytes.len(), self.get()));
        }
        // SAFETY: the check above bounds this slice by the same ceiling.
        Ok(unsafe { self.project(bytes) })
    }
}
