use std::{marker, pin};

/// A value accessible only with the matching application [`Token`].
pub use crate::base::cell::values::Value;

use crate::base::cell::{Sealed, region};

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
pub use crate::base::cell::values::Token;

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
    f(Token::new(), (Token::new(), Token::new()), root)
}

const _: () = {
    assert!(size_of::<Token<Identity<'static>>>() == 0);
    assert!(size_of::<Value<Identity<'static>, u64>>() == size_of::<u64>());
    assert!(align_of::<Value<Identity<'static>, u64>>() == align_of::<u64>());
};
