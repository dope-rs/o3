use std::{marker, pin};

pub use value::Value;

use crate::base::cell::{Sealed, brand, region, values};

mod value;

/// One generative application identity, invariant and unforgeable.
///
/// The trait is sealed: [`Identity`] is the only brand this crate issues, so no
/// consumer can implement a brand or forge one outside the generative scopes.
pub trait Brand: Sealed {}

/// The invariant identity of one generative application domain.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Identity<'id>(marker::PhantomData<*mut &'id ()>);

impl<'id> Sealed for Identity<'id> {}

impl<'id> Brand for Identity<'id> {}

/// Exclusive permission for one generative application domain.
#[repr(transparent)]
pub struct Token<B: Brand> {
    pub(in crate::base::cell) token: values::Token<B>,
}

impl<B: Brand> Token<B> {
    pub const fn bind<T>(&self, value: T) -> brand::Value<B, T> {
        brand::Value {
            value: values::Value::new(value),
        }
    }
}

impl Token<Identity<'_>> {
    /// Creates matching domains and lends one pinned root for their lifetime.
    pub fn scope_pinned<T: ?Sized, R>(
        root: pin::Pin<&mut T>,
        f: impl for<'id> FnOnce(
            Token<Identity<'id>>,
            (
                region::Token<region::Identity<'id>>,
                region::Token<region::Identity<'id, 1>>,
            ),
            pin::Pin<&'id mut T>,
        ) -> R,
    ) -> R {
        f(
            Token {
                token: values::Token::new(),
            },
            (values::Token::new().into(), values::Token::new().into()),
            root,
        )
    }
}
