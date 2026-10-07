use std::mem;

use crate::mem::credit::fair;

/// One affine acquisition retaining both its issuing pool and state borrows.
#[must_use = "a detached grant must be stored or returned"]
pub struct Grant<
    S: crate::mem::raw::Source<N>,
    const N: usize = 1,
    A: crate::mem::raw::Amount<N, S::Word> = [<S as crate::mem::raw::Source<N>>::Word; N],
> {
    charge: mem::ManuallyDrop<crate::mem::raw::Charge<S, N, A>>,
}

impl<S: crate::mem::raw::Source<N>, const N: usize, A: crate::mem::raw::Amount<N, S::Word>>
    From<crate::mem::raw::Charge<S, N, A>> for Grant<S, N, A>
{
    fn from(charge: crate::mem::raw::Charge<S, N, A>) -> Self {
        Self {
            charge: mem::ManuallyDrop::new(charge),
        }
    }
}

impl<S: crate::mem::raw::Source<N>, const N: usize, A: crate::mem::raw::Amount<N, S::Word>>
    Grant<S, N, A>
{
    pub fn amount(&self) -> A {
        self.charge.amount
    }

    /// Expands the exact component vector without changing its issuing accounts.
    pub fn into_array(self) -> Grant<S, N> {
        Grant::from(crate::mem::raw::Charge::new(
            self.charge.source,
            self.charge.amount.get(),
        ))
    }

    /// Acquires additional credit from this grant's issuing source and retains
    /// its exclusive target until the addition is committed or refunded.
    pub fn try_extend(&mut self, amount: A) -> Extension<'_, S, N, A>
    where
        A: crate::mem::raw::Sum<N, S::Word>,
    {
        let charge = crate::mem::raw::Charge::try_new(self.charge.source, amount)?;
        Ok(crate::mem::raw::Bound {
            source: self,
            proof: charge,
        })
    }

    pub(super) unsafe fn merge_bound(&mut self, other: Self)
    where
        A: crate::mem::raw::Sum<N, S::Word>,
    {
        // SAFETY: the bound pair owns disjoint acquisitions in the same lane,
        // whose admitted held total bounds their componentwise sum.
        self.charge.amount = unsafe { A::add_raw(self.charge.amount, other.charge.amount) };
    }

    /// Returns the acquisition to its retained issuing pool and lane.
    pub fn release(self) {
        drop(mem::ManuallyDrop::into_inner(self.charge));
    }

    /// # Safety
    /// All ExactRebind/Exact operations must preserve this grant's components for every input.
    pub unsafe fn bind<P>(self, proof: P) -> crate::mem::raw::Bound<Self, P> {
        crate::mem::raw::Bound {
            source: self,
            proof,
        }
    }

    ///
    /// # Safety
    /// All Exact operations must partition this grant's components for every input.
    /// A bound Grant or Charge must own a disjoint acquisition from the same pool and state.
    pub unsafe fn bind_mut<P>(&mut self, proof: P) -> crate::mem::raw::Bound<&mut Self, P> {
        crate::mem::raw::Bound {
            source: self,
            proof,
        }
    }

    /// Applies a representation change proved exact for every source value.
    pub fn map<P>(self, change: P) -> Grant<S, N, P::Output>
    where
        P: crate::mem::raw::Rebind<N, S::Word, A>,
    {
        // SAFETY: Rebind proves preservation for every possible source amount.
        unsafe { self.map_exact(change) }
    }

    /// Changes representation-only data using a universally exact operation.
    pub fn update<P>(&mut self, change: P)
    where
        P: crate::mem::raw::Rebind<N, S::Word, A, Output = A>,
    {
        self.charge.amount = change.rebind(self.charge.amount);
    }

    pub(super) unsafe fn into_parts<P>(
        self,
        proof: P,
        input: P::Input,
    ) -> crate::mem::raw::Partition<S, N, A, P>
    where
        P: crate::mem::raw::Exact<N, S::Word, A>,
    {
        let (left, right) = proof.parts(self.charge.amount, input);
        let source = self.charge.source;
        (
            Grant::from(crate::mem::raw::Charge::new(source, left)),
            Grant::from(crate::mem::raw::Charge::new(source, right)),
        )
    }

    /// Changes representation after a caller proves the exact source value.
    /// # Safety
    /// `proof` must preserve this grant's complete component vector.
    pub(super) unsafe fn map_exact<P>(self, proof: P) -> Grant<S, N, P::Output>
    where
        P: crate::mem::raw::ExactRebind<N, S::Word, A>,
    {
        Grant::from(crate::mem::raw::Charge::new(
            self.charge.source,
            proof.rebind(self.charge.amount),
        ))
    }

    /// Removes one exact part after a semantic owner has checked the split.
    /// # Safety
    /// The marker must hold the exact source-bound partition proof.
    pub(super) unsafe fn split_exact<P>(
        &mut self,
        proof: P,
        input: P::Input,
    ) -> Grant<S, N, P::Right>
    where
        P: crate::mem::raw::Exact<N, S::Word, A, Left = A>,
    {
        let (rest, part) = proof.parts(self.charge.amount, input);
        self.charge.amount = rest;
        Grant::from(crate::mem::raw::Charge::new(self.charge.source, part))
    }
}

const _: () = {
    assert!(
        mem::size_of::<Grant<crate::mem::raw::Lane<'static, 'static>>>()
            == 3 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Grant<crate::mem::raw::Lane<'static, 'static, 2>, 2>>()
            == 4 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Grant<crate::mem::raw::Lane<'static, 'static, 2, (), u32>, 2>>()
            == 3 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<Grant<crate::mem::raw::Lane<'static, 'static, 2, (), u32>, 2>>>()
            == mem::size_of::<Grant<crate::mem::raw::Lane<'static, 'static, 2, (), u32>, 2>>()
    );
    assert!(!mem::needs_drop::<
        Grant<crate::mem::raw::Lane<'static, 'static, 2, (), u32>, 2>,
    >());
};

impl<S: crate::mem::raw::Source<N>, const N: usize> Grant<S, N> {
    /// Moves the complete acquisition out, retaining an empty same-source grant.
    pub fn drain(&mut self) -> Self {
        let empty = Self::empty(self.charge.source);
        mem::replace(self, empty)
    }

    /// Starts a zero acquisition in this exact source without changing credit.
    pub fn empty(source: S) -> Self {
        Self::from(crate::mem::raw::Charge::new(
            source,
            [<S::Word as crate::mem::raw::Word>::ZERO; N],
        ))
    }
}

/// Extension retains the exclusive target and refunds its newly acquired charge until commit.
type Extension<'a, S, const N: usize, A> = Result<
    crate::mem::raw::Bound<&'a mut Grant<S, N, A>, crate::mem::raw::Charge<S, N, A>>,
    crate::mem::Rejected<A, fair::Cause<<S as crate::mem::raw::Source<N>>::Word, N>>,
>;
