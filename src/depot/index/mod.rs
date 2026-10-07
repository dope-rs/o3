use std::marker;

use crate::depot;

pub mod bitmap;
mod domain;
mod ordinal;

pub use domain::Domain;
pub use ordinal::Ordinal;

pub mod set;

mod stored;
pub use stored::Stored;

/// Safe projections of a capability retaining one initialized slot.
pub trait Access<P = ()>: crate::depot::raw::Source<P> {
    fn into_live(self) -> crate::depot::raw::Proof<(Self, Self::Key), Initialized<P>> {
        let key = self.key();
        // SAFETY: consuming the nominal retains its complete owner and proof.
        unsafe { crate::depot::raw::Proof::new((self, key)) }
    }

    fn read(&self) -> crate::depot::raw::Proof<(&Self, Self::Key), Initialized<P>> {
        // SAFETY: the nominal source retains its checked slot and operation.
        unsafe { crate::depot::raw::Proof::new((self, self.key())) }
    }
}

impl<S: crate::depot::raw::Source<P>, P> Access<P> for S {}

/// The retained (owner, key) pair names an initialized slot in that exact owner and plane.
/// Every ownership, lifetime, and state condition of the operation P is retained.
pub struct Initialized<P>(marker::PhantomData<fn(P) -> P>);

/// The retained owner excludes writes to this vacant cell until initialization.
pub struct Vacant<T>(marker::PhantomData<fn(T) -> T>);

impl<P> depot::Predicate for Initialized<P> {}

/// The retained (owner, key) pair names an initialized slot whose exact owner
/// holds no live borrow of it, so interior admission needs no busy probe.
pub struct Unborrowed<P>(marker::PhantomData<fn(P) -> P>);

impl<P> depot::Predicate for Unborrowed<P> {}

/// A native dense ordinal whose maximum representation is the absent sentinel.
pub trait Position:
    crate::depot::raw::Word + crate::depot::raw::Link<Item = Self> + Ord + crate::depot::raw::Numeric
{
}

impl Position for u16 {}
impl Position for u32 {}
