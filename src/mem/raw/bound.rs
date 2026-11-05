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
