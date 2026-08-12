//! The application-domain capability proof.

use std::{cell, marker, pin};

use crate::cell::region;

type Invariant<'id> = marker::PhantomData<*mut &'id ()>;

/// Exclusive permission for one generative application domain.
#[repr(transparent)]
pub struct Token<'id> {
    brand: Invariant<'id>,
}

impl Token<'_> {
    /// Creates one application domain that cannot escape `operation`.
    pub fn scope<R>(operation: impl for<'id> FnOnce(Token<'id>) -> R) -> R {
        operation(Token::new())
    }

    /// Creates independent application and state domains in one scope.
    pub fn scope_with_region<R>(
        operation: impl for<'id> FnOnce(Token<'id>, region::Token<'id>) -> R,
    ) -> R {
        operation(Token::new(), region::Token::new())
    }

    const fn new() -> Self {
        Self {
            brand: marker::PhantomData,
        }
    }
}

/// A value accessible only with the matching application [`Token`].
#[repr(transparent)]
pub struct Value<'id, T> {
    value: cell::UnsafeCell<T>,
    brand: Invariant<'id>,
}

impl<'id, T> Value<'id, T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: cell::UnsafeCell::new(value),
            brand: marker::PhantomData,
        }
    }

    pub fn borrow<'a>(&'a self, token: &'a Token<'id>) -> &'a T {
        let _ = token;
        // SAFETY: a shared token can coexist only with other shared borrows in
        // the same generative domain. Safe code cannot mint a matching token.
        unsafe { &*self.value.get() }
    }

    pub fn borrow_mut<'a>(&'a self, token: &'a mut Token<'id>) -> &'a mut T
    where
        T: Unpin,
    {
        let _ = token;
        // SAFETY: the unique token borrow proves exclusive access to every
        // value in this generative domain for the returned borrow's lifetime.
        unsafe { &mut *self.value.get() }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }

    pub fn borrow_pin_mut<'a>(
        self: pin::Pin<&'a Self>,
        token: &'a mut Token<'id>,
    ) -> pin::Pin<&'a mut T> {
        let _ = token;
        // SAFETY: the unique token proves exclusive access and the enclosing
        // transparent Value is pinned, so its UnsafeCell payload cannot move.
        unsafe { pin::Pin::new_unchecked(&mut *self.get_ref().value.get()) }
    }

    pub fn borrow_pin<'a>(self: pin::Pin<&'a Self>, token: &'a Token<'id>) -> pin::Pin<&'a T> {
        // SAFETY: the shared token proves shared access and the enclosing
        // transparent Value is pinned, so its UnsafeCell payload cannot move.
        unsafe { pin::Pin::new_unchecked(self.get_ref().borrow(token)) }
    }
}
