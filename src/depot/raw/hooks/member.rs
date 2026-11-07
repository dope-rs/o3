use crate::depot::queue::links;

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
