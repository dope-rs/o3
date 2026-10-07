//! Values and proofs independent of allocation and resource ownership.

use std::convert::Infallible;

pub mod cell;
pub mod marker;
pub mod num;
pub mod time;

/// Consumes one callback through static dispatch with its exact argument type.
pub trait Call<T> {
    fn call(self, args: T);
}

impl<T, F: FnOnce(T)> Call<T> for F {
    fn call(self, args: T) {
        self(args);
    }
}

impl<T> Call<T> for Infallible {
    fn call(self, _: T) {
        match self {}
    }
}
