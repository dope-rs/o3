use crate::base::cell::region;

/// Retains one source together with the proof authorizing its operations.
#[derive(Clone, Copy, Debug)]
#[must_use = "the bound transformation retains its unchanged source"]
pub struct Bound<S, P> {
    pub(super) source: S,
    pub(super) proof: P,
}

impl<'d, T> Bound<&'d T, region::Proof<region::Identity<'d>>> {
    pub(crate) const fn new(source: &'d T, proof: region::Proof<region::Identity<'d>>) -> Self {
        Self { source, proof }
    }

    pub(crate) const fn source(&self) -> &'d T {
        self.source
    }
}

impl<S, const N: usize, A, P> Bound<crate::mem::raw::Grant<S, N, A>, P>
where
    S: crate::mem::raw::Source<N>,
    A: crate::mem::raw::Amount<N, S::Word>,
{
    pub fn map(self) -> crate::mem::raw::Grant<S, N, P::Output>
    where
        P: crate::mem::raw::ExactRebind<N, S::Word, A>,
    {
        // SAFETY: the affine bound marker owns its unchanged exact source.
        unsafe { self.source.map_exact(self.proof) }
    }

    pub fn split(self, input: P::Input) -> crate::mem::raw::Partition<S, N, A, P>
    where
        P: crate::mem::raw::Exact<N, S::Word, A>,
    {
        // SAFETY: binding proves this partition for the retained exact source.
        unsafe { self.source.into_parts(self.proof, input) }
    }
}

impl<S, const N: usize, A, P> Bound<&mut crate::mem::raw::Grant<S, N, A>, P>
where
    S: crate::mem::raw::Source<N>,
    A: crate::mem::raw::Amount<N, S::Word>,
    P: crate::mem::raw::Exact<N, S::Word, A, Left = A>,
{
    pub fn split(self, input: P::Input) -> crate::mem::raw::Grant<S, N, P::Right> {
        // SAFETY: the exclusive bound borrow prevents replacement or mutation
        // of the checked source until this exact partition is consumed.
        unsafe { self.source.split_exact(self.proof, input) }
    }
}

impl<S, const N: usize, A>
    Bound<&mut crate::mem::raw::Grant<S, N, A>, crate::mem::raw::Grant<S, N, A>>
where
    A: crate::mem::raw::Sum<N, S::Word>,
    S: crate::mem::raw::Source<N>,
{
    /// Consumes the source-bound addition without checking the origin again.
    pub fn merge(self) {
        // SAFETY: binding retains both exact same-source affine acquisitions.
        unsafe { self.source.merge_bound(self.proof) };
    }
}

impl<S, const N: usize, A>
    Bound<&mut crate::mem::raw::Grant<S, N, A>, crate::mem::raw::Charge<S, N, A>>
where
    A: crate::mem::raw::Sum<N, S::Word>,
    S: crate::mem::raw::Source<N>,
{
    /// Commits the addition; dropping the unconsumed bound refunds its charge.
    pub fn merge(self) {
        // SAFETY: try_extend issued this charge from the retained target itself.
        unsafe {
            self.source
                .merge_bound(crate::mem::raw::Grant::from(self.proof))
        };
    }
}
