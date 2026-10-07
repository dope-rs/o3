use std::{marker, pin};

pub use proof::Proof;
pub use value::Value;

use crate::base::cell::{Sealed, region, values};

mod proof;
mod value;

/// One generative state identity, invariant and unforgeable.
///
/// The trait is sealed: [`Identity`] is the only brand this crate issues, so no
/// consumer can implement a brand or forge one outside the generative scopes.
pub trait Brand: Sealed {}

/// The invariant identity of one generative region.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Identity<'id, const ID: u8 = 0>(marker::PhantomData<*mut &'id ()>);

impl<'id, const ID: u8> Sealed for Identity<'id, ID> {}

impl<'id, const ID: u8> Brand for Identity<'id, ID> {}

/// Exclusive permission for one generative state domain.
#[repr(transparent)]
pub struct Token<B: Brand> {
    token: values::Token<B>,
}

impl<B: Brand> Token<B> {
    pub const fn bind<T>(&self, value: T) -> region::Value<B, T> {
        self.proof().bind(value)
    }

    pub fn borrow<'a, T>(&'a self, value: &'a region::Value<B, T>) -> &'a T {
        self.token.borrow(&value.value)
    }

    pub fn borrow_mut<'a, T: Unpin>(&'a mut self, value: &'a region::Value<B, T>) -> &'a mut T {
        self.token.borrow_mut(&value.value)
    }

    /// Copies the opaque identity proof for capabilities owned by this region.
    pub const fn proof(&self) -> region::Proof<B> {
        region::Proof {
            proof: self.token.proof(),
        }
    }
}

impl Token<Identity<'_>> {
    /// Creates one state domain that cannot escape the operation.
    pub fn scope<R>(f: impl for<'id> FnOnce(Token<Identity<'id>>) -> R) -> R {
        f(values::Token::new().into())
    }

    /// Binds one state domain to the full borrow of its pinned root.
    pub fn scope_pinned<T: ?Sized, R>(
        root: pin::Pin<&mut T>,
        f: impl for<'id> FnOnce(Token<Identity<'id>>, pin::Pin<&'id mut T>) -> R,
    ) -> R {
        f(values::Token::new().into(), root)
    }
}

impl<B: Brand> From<values::Token<B>> for Token<B> {
    fn from(token: values::Token<B>) -> Self {
        Self { token }
    }
}
