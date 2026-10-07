use crate::depot::queue::links;
/// Proof that one exact hook is detached from every topology.
pub struct Detached<'a> {
    hook: &'a links::Hook,
    index: links::Index,
}

impl Detached<'_> {
    /// The compact identity of this retained detached hook.
    pub const fn index(&self) -> links::Index {
        self.index
    }

    /// Appends this detached hook to its owner's reciprocal chain.
    /// # Safety
    /// This hook and the chain's reciprocal endpoints belong to `hooks`.
    pub(in crate::depot) unsafe fn push<H: crate::depot::raw::Hooks + ?Sized>(
        self,
        chain: &links::Chain,
        hooks: &H,
    ) {
        // SAFETY: the caller retains the same store and reciprocal endpoints.
        let change =
            unsafe { crate::depot::raw::Insert::<(), false>::from_vacant(chain, hooks, self) };
        change.commit();
    }

    pub fn idle(self) {
        self.hook.previous.set(self.index.raw());
        self.hook.next.set(links::Index::END);
    }

    pub fn park(self) {
        self.hook.previous.set(self.index.raw());
        self.hook.next.set(self.index.raw());
    }
}
impl<'a> Detached<'a> {
    pub fn from_source(
        proof: crate::depot::raw::Proof<(&'a links::Hook, links::Index), links::Vacant>,
    ) -> Self {
        let (hook, index) = proof.into_inner();
        Self { hook, index }
    }

    /// # Safety
    /// The exact hook is exclusively detached at this index with no other live capability.
    pub(in crate::depot) unsafe fn new(hook: &'a links::Hook, index: links::Index) -> Self {
        Self { hook, index }
    }

    pub(in crate::depot) fn into_parts(self) -> (&'a links::Hook, links::Index) {
        (self.hook, self.index)
    }
}
