use std::marker;

/// Maximum resident backing retained by one owner.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    AtMost(usize),
    Unbounded,
}

/// Type-level resident backing bound.
pub trait Bound {
    const LIMIT: Limit;
}

/// Proof of a finite resident backing bound.
pub enum AtMost<const N: usize> {}

impl<const N: usize> Bound for AtMost<N> {
    const LIMIT: Limit = Limit::AtMost(N);
}

/// Proof that resident backing has no finite static bound.
pub enum Unbounded {}

impl Bound for Unbounded {
    const LIMIT: Limit = Limit::Unbounded;
}

/// Bound covering either of two retained owner shapes.
pub struct Max<L, R>(marker::PhantomData<fn() -> (L, R)>);

impl<L: Bound, R: Bound> Bound for Max<L, R> {
    const LIMIT: Limit = match (L::LIMIT, R::LIMIT) {
        (Limit::AtMost(left), Limit::AtMost(right)) => {
            Limit::AtMost(if left > right { left } else { right })
        }
        (Limit::Unbounded, _) | (_, Limit::Unbounded) => Limit::Unbounded,
    };
}
