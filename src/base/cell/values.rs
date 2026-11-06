use std::{cell, marker, pin};

use crate::base::cell::{Sealed, brand, region};

/// Exclusive access issued only by a generative scope.
#[repr(transparent)]
pub struct Token<B: Sealed> {
    proof: marker::PhantomData<B>,
}

/// One cell whose access permission belongs to exactly one brand.
#[derive(Debug)]
#[repr(transparent)]
pub struct Value<B: Sealed, T> {
    value: cell::UnsafeCell<T>,
    brand: marker::PhantomData<B>,
}

impl<B: Sealed> Token<B> {
    pub(in crate::base::cell) const fn new() -> Self {
        Self {
            proof: marker::PhantomData,
        }
    }

    pub const fn bind<T>(&self, value: T) -> Value<B, T> {
        Value::new(value)
    }

    pub(in crate::base::cell) fn read<'a, T>(&'a self, value: &'a Value<B, T>) -> &'a T {
        // SAFETY: the invariant brand matches this unique domain token; shared
        // token access prevents an exclusive borrow for this lifetime.
        unsafe { &*value.value.get() }
    }
}

impl<B: region::Brand> Token<B> {
    pub fn borrow_mut<'a, T: Unpin>(&'a mut self, value: &'a Value<B, T>) -> &'a mut T {
        // SAFETY: the matching brand identity and exclusive token exclude every
        // other access. Unpin permits replacement of the value.
        unsafe { &mut *value.value.get() }
    }
}

impl<B: brand::Brand> brand::Token<B> {
    pub fn pin_mut<'a, T>(
        &'a mut self,
        value: pin::Pin<&'a brand::Value<B, T>>,
    ) -> pin::Pin<&'a mut T> {
        // SAFETY: the matching exclusive token excludes every other access.
        // Value structurally pins its private storage and T.
        unsafe { pin::Pin::new_unchecked(&mut *value.value.get()) }
    }

    pub fn borrow_pin<'a, T>(&'a self, value: pin::Pin<&'a brand::Value<B, T>>) -> pin::Pin<&'a T> {
        // SAFETY: Value structurally pins T. Shared token access
        // excludes mutable access throughout the returned borrow.
        unsafe { pin::Pin::new_unchecked(self.read(value.get_ref())) }
    }
}

impl<B: Sealed, T> Value<B, T> {
    pub(in crate::base::cell) const fn new(value: T) -> Self {
        Self {
            value: cell::UnsafeCell::new(value),
            brand: marker::PhantomData,
        }
    }
}

impl<B: region::Brand, T> region::Value<B, T> {
    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
}
