use std::marker;

use crate::mem::credit::fair;

/// A provisional acquisition that refunds unless detached into a grant.
#[must_use = "dropping the charge returns its fair credit"]
pub struct Charge<
    S: crate::mem::raw::Source<N>,
    const N: usize = 1,
    A: crate::mem::raw::Amount<N, S::Word> = [<S as crate::mem::raw::Source<N>>::Word; N],
> {
    pub(super) source: S,
    pub(super) amount: A,
    owner: crate::mem::raw::Domain<S::Tag, S::Word>,
}

impl<S, const N: usize, A> Charge<S, N, A>
where
    A: crate::mem::raw::Amount<N, S::Word>,
    S: crate::mem::raw::Source<N>,
{
    pub(super) const fn new(source: S, amount: A) -> Self {
        Self {
            source,
            amount,
            owner: marker::PhantomData,
        }
    }

    /// Acquires from the immutable source retained by the resulting charge.
    pub fn try_new(
        source: S,
        amount: A,
    ) -> Result<Self, crate::mem::Rejected<A, fair::Cause<S::Word, N>>> {
        let raw = amount.get();
        // SAFETY: the checked admission below commits exactly one acquisition.
        let (shared, state) = unsafe { source.accounts() };
        let balance = crate::mem::raw::Balance::new(state.held.get(), shared.get());
        let balance = balance
            .try_acquire(state.reserve, raw)
            .map_err(|denial| crate::mem::Rejected::new(amount, denial))?;
        let (next, surplus) = balance.into_parts();
        state.held.set(next);
        shared.set(surplus);
        Ok(Self::new(source, amount))
    }
}

impl<S, const N: usize, A> Drop for Charge<S, N, A>
where
    A: crate::mem::raw::Amount<N, S::Word>,
    S: crate::mem::raw::Source<N>,
{
    fn drop(&mut self) {
        let amount = self.amount.get();
        // SAFETY: this charge owns the exact acquisition returned below.
        let (shared, state) = unsafe { self.source.accounts() };
        let balance = crate::mem::raw::Balance::new(state.held.get(), shared.get());
        // SAFETY: the charge owns its exact refundable acquisition and its
        // immutable source keeps that issuing pool and lane inseparable.
        let (next, surplus) = unsafe { balance.release_raw(state.reserve, amount) }.into_parts();
        state.held.set(next);
        shared.set(surplus);
    }
}

const _: () =
    assert!(size_of::<Charge<crate::mem::raw::Lane<'static, 'static>>>() == size_of::<usize>() * 3);
const _: () = assert!(
    size_of::<Charge<crate::mem::raw::Lane<'static, 'static, 2>, 2>>() == size_of::<usize>() * 4
);
