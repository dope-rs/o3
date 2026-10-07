use std::num;

/// One exact policy charge request.
#[derive(Debug)]
pub struct Debit<P: crate::buffer::raw::Policy> {
    seed: P::Seed,
    amount: num::NonZero<u32>,
}

impl<P: crate::buffer::raw::Policy> Debit<P> {
    pub(in crate::buffer) const fn new(seed: P::Seed, amount: num::NonZero<u32>) -> Self {
        Self { seed, amount }
    }

    pub(in crate::buffer) fn grow(policy: &P, amount: num::NonZero<u32>) -> Self {
        Self::new(policy.seed(), amount)
    }

    pub(in crate::buffer) const fn amount(&self) -> num::NonZero<u32> {
        self.amount
    }

    pub(in crate::buffer) const fn seed(&self) -> P::Seed {
        self.seed
    }
}

pub trait Token<P: crate::buffer::raw::Policy>: Sized {
    fn debit(&self) -> &Debit<P>;

    fn commit(self) -> (P, num::NonZero<u32>);
}

impl Token<()> for Debit<()> {
    fn debit(&self) -> &Self {
        self
    }

    fn commit(self) -> ((), num::NonZero<u32>) {
        (self.seed, self.amount)
    }
}

const _: () = assert!(size_of::<Debit<()>>() == size_of::<num::NonZero<u32>>());
