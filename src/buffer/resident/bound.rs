use std::{convert::Infallible, marker};

/// Maximum resident backing retained by one owner.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Limit<F> {
    AtMost(usize, F),
    Unbounded,
}

/// Type-level resident backing bound.
pub trait Bound {
    type Finite: Copy;
    type Stored<'a, T: 'a>: 'a;

    const LIMIT: Limit<Self::Finite>;

    fn store<'a, T: 'a>(proof: Self::Finite, value: T) -> Self::Stored<'a, T>;
    fn get_mut<'a: 'b, 'b, T: 'a>(value: &'b mut Self::Stored<'a, T>) -> &'b mut T;
    fn into_inner<'a, T: 'a>(value: Self::Stored<'a, T>) -> T;
}

/// Proof of a finite resident backing bound.
pub enum AtMost<const N: usize> {}

impl<const N: usize> Bound for AtMost<N> {
    type Finite = ();
    type Stored<'a, T: 'a> = T;

    const LIMIT: Limit<Self::Finite> = Limit::AtMost(N, ());

    fn store<'a, T: 'a>((): Self::Finite, value: T) -> Self::Stored<'a, T> {
        value
    }

    fn get_mut<'a: 'b, 'b, T: 'a>(value: &'b mut Self::Stored<'a, T>) -> &'b mut T {
        value
    }

    fn into_inner<'a, T: 'a>(value: Self::Stored<'a, T>) -> T {
        value
    }
}

/// Proof that resident backing has no finite static bound.
pub enum Unbounded {}

impl Bound for Unbounded {
    type Finite = Infallible;
    type Stored<'a, T: 'a> = Infallible;

    const LIMIT: Limit<Self::Finite> = Limit::Unbounded;

    fn store<'a, T: 'a>(proof: Self::Finite, _: T) -> Self::Stored<'a, T> {
        match proof {}
    }

    fn get_mut<'a: 'b, 'b, T: 'a>(value: &'b mut Self::Stored<'a, T>) -> &'b mut T {
        match *value {}
    }

    fn into_inner<'a, T: 'a>(value: Self::Stored<'a, T>) -> T {
        match value {}
    }
}

/// Bound covering either of two retained owner shapes.
pub struct Max<L, R>(marker::PhantomData<fn() -> (L, R)>);

impl<L: Bound, R: Bound> Bound for Max<L, R> {
    type Finite = (L::Finite, R::Finite);
    type Stored<'a, T: 'a> = L::Stored<'a, R::Stored<'a, T>>;

    const LIMIT: Limit<Self::Finite> = match (L::LIMIT, R::LIMIT) {
        (Limit::AtMost(left, l), Limit::AtMost(right, r)) => {
            Limit::AtMost(if left > right { left } else { right }, (l, r))
        }
        (Limit::Unbounded, _) | (_, Limit::Unbounded) => Limit::Unbounded,
    };

    fn store<'a, T: 'a>((left, right): Self::Finite, value: T) -> Self::Stored<'a, T> {
        L::store(left, R::store(right, value))
    }

    fn get_mut<'a: 'b, 'b, T: 'a>(value: &'b mut Self::Stored<'a, T>) -> &'b mut T {
        R::get_mut::<T>(L::get_mut::<R::Stored<'a, T>>(value))
    }

    fn into_inner<'a, T: 'a>(value: Self::Stored<'a, T>) -> T {
        R::into_inner::<T>(L::into_inner::<R::Stored<'a, T>>(value))
    }
}
