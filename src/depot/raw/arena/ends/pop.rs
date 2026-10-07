use crate::depot;

/// # Safety
/// The implementation updates only its original native endpoint owner.
pub unsafe trait EndsPop<L: depot::raw::Link> {
    /// # Safety
    /// The stable front is current; its exact callback returns its stored successor without reentry.
    unsafe fn pop<T, R>(
        self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R;
}
