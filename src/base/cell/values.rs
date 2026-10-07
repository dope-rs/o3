use std::{cell, marker, pin};

use crate::base::cell::{Sealed, brand};

/// Exclusive access issued only by a generative scope.
#[repr(transparent)]
pub(in crate::base::cell) struct Token<B: Sealed> {
    proof: Proof<B>,
}

/// Invariant identity without access permission.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub(in crate::base::cell) struct Proof<B: Sealed> {
    brand: marker::PhantomData<B>,
}

/// One cell whose access permission belongs to exactly one brand.
#[derive(Debug)]
#[repr(transparent)]
pub(in crate::base::cell) struct Value<B: Sealed, T> {
    value: cell::UnsafeCell<T>,
    brand: marker::PhantomData<B>,
}

impl<B: Sealed> Token<B> {
    pub(in crate::base::cell) const fn new() -> Self {
        Self {
            proof: Proof {
                brand: marker::PhantomData,
            },
        }
    }

    pub(in crate::base::cell) const fn proof(&self) -> Proof<B> {
        self.proof
    }

    pub(in crate::base::cell) fn borrow<'a, T>(&'a self, value: &'a Value<B, T>) -> &'a T {
        // SAFETY: the invariant brand matches this unique domain token; shared
        // token access prevents an exclusive borrow for this lifetime.
        unsafe { &*value.value.get() }
    }

    pub(in crate::base::cell) fn borrow_mut<'a, T: Unpin>(
        &'a mut self,
        value: &'a Value<B, T>,
    ) -> &'a mut T {
        // SAFETY: the matching brand identity and exclusive token exclude every
        // other access. Unpin permits replacement of the value.
        unsafe { &mut *value.value.get() }
    }
}

impl<B: brand::Brand> Token<B> {
    pub(in crate::base::cell) fn pin_mut<'a, T>(
        &'a mut self,
        value: pin::Pin<&'a brand::Value<B, T>>,
    ) -> pin::Pin<&'a mut T> {
        // SAFETY: the matching exclusive token excludes every other access.
        // Both Value layers structurally pin their private storage and T.
        unsafe { pin::Pin::new_unchecked(&mut *value.value.value.get()) }
    }

    pub(in crate::base::cell) fn borrow_pin<'a, T>(
        &'a self,
        value: pin::Pin<&'a brand::Value<B, T>>,
    ) -> pin::Pin<&'a T> {
        // SAFETY: both Value layers structurally pin T. Shared token access
        // excludes mutable access throughout the returned borrow.
        unsafe { pin::Pin::new_unchecked(self.borrow(&value.get_ref().value)) }
    }
}

impl<B: Sealed> Proof<B> {
    pub(in crate::base::cell) const fn bind<T>(self, value: T) -> Value<B, T> {
        Value {
            value: cell::UnsafeCell::new(value),
            brand: self.brand,
        }
    }
}

impl<B: Sealed, T> Value<B, T> {
    pub(in crate::base::cell) fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub(in crate::base::cell) fn into_inner(self) -> T {
        self.value.into_inner()
    }
}
