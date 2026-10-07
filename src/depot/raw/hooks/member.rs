use crate::depot::queue::links;

/// Consumes one exact owner and its admitted FIFO membership phase.
/// # Safety
/// Parts retain exclusive mutation of one reciprocal chain/store/index in phase `LINKED`; projections and their destruction return normally without changing that phase.
pub(in crate::depot) unsafe trait Membership<const LINKED: bool>:
    Sized
{
    type Output;

    fn parts(
        &self,
    ) -> (
        &links::Chain<u32>,
        impl crate::depot::raw::Hooks + '_,
        links::Index,
    );

    /// # Safety
    /// The exact retained hook has transitioned to the opposite membership phase.
    unsafe fn finish(self) -> Self::Output;

    fn transition(self) -> Self::Output {
        {
            let (queue, hooks, index) = self.parts();
            // SAFETY: Membership retains the same reciprocal topology and the admitted phase.
            unsafe {
                if LINKED {
                    queue.unlink(&hooks, index).idle();
                } else {
                    queue.link_back(&hooks, index);
                }
            }
        }
        // SAFETY: the single transition changed this exact owner's hook to the opposite phase.
        unsafe { self.finish() }
    }
}

/// A prepared removal retaining its exact nonempty chain.
/// # Safety
/// This operation owns a linked hook until its consuming commit.
pub(in crate::depot) unsafe trait Removal<'a, C: crate::depot::raw::ChainCount>:
    Sized
{
    fn links(&self) -> &'a links::Chain<C>;
    /// # Safety
    /// The consuming count owner commits this exact removal's cardinality change.
    unsafe fn apply(self) -> links::Detached<'a>;

    /// Commits the prepared live member, which proves this exact chain's count is nonzero.
    fn commit(self) -> links::Detached<'a>
    where
        C: 'a,
    {
        let count = C::storage((unsafe { self.links().parts() }).2);
        let next = count.map(|count| count.get() - 1);
        let vacant = unsafe { self.apply() };
        if let Some((count, next)) = count.zip(next) {
            count.set(next);
        }
        vacant
    }
}
