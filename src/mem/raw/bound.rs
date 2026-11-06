/// Retains one source together with the proof authorizing its operations.
#[derive(Clone, Copy, Debug)]
#[must_use = "the bound transformation retains its unchanged source"]
pub struct Bound<S, P> {
    pub(super) source: S,
    pub(super) proof: P,
}
